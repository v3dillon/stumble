# Stumble

Stumble is a decentralized personal discovery system that assembles a User's Feed from independently curated Pods on their Home Node. Agent Harnesses operate it through the CLI, HTTP, or MCP.

## Get started

Install [Rust](https://rustup.rs), clone this repository, then run:

```bash
cargo install --path crates/stumble-cli --locked
stumble node init
stumble node show
```

`stumble` uses `~/.stumble/nodes/home` by default. Set `STUMBLE_DATA_DIR`
or pass `--data-dir` to select another Home Node. Only `node init` creates
state; it stores the Home Node Owner Credential in the operating-system
credential store, and later local commands retrieve it automatically.

For agent-assisted setup, ask your agent to read [llms.txt](llms.txt) and install Stumble.

To start the optional local HTTP API, run
`stumble-api --data-dir ~/.stumble/nodes/home`. Long-running HTTP and MCP
transports are separate from the one-shot `stumble` workflow CLI.

## Workflow CLI

`stumble` is a local, JSON-first workflow CLI with exactly five top-level families: `node`, `pod`, `discover`, `feed`, and `sync`. Success writes one `{ "version": 1, "data": ... }` document to standard output; failures write one versioned error document to standard error. Scoped automation supplies `STUMBLE_HARNESS_CREDENTIAL`; local User commands retrieve the Home Node Owner Credential automatically.

### Harnesses and approvals

- `stumble node harness register` — Owner-only bootstrap that activates a scoped Harness Grant and returns its credential once. Flags: `--label`, `--kind <interactive|unattended>`, repeatable `--capability`, and repeatable `--pod-id`.
- `stumble node harness list|show|revoke` — Inspects credential fingerprints and metadata or immediately revokes a Harness. Plaintext credentials are never returned by reads.
- `stumble node proposal list|show|approve|reject` — Reviews expiring Pending Proposals. Approval and rejection require either the automatically authenticated Owner or an independent interactive Harness with approval capability and matching User and Pod scope.

Local Owner commands authenticate from the Home Node Owner Credential automatically. Scoped automation supplies `STUMBLE_HARNESS_CREDENTIAL`; authority expansion creates a Pending Proposal and never changes the Harness Grant before approval. Generic proposal creation and tenant or raw-token administration are not CLI workflows.

Subscription and Pod authority are separate workflows. Use an exact local slug or immutable Pod ID with `stumble pod subscribe`, `pod unsubscribe`, and `pod subscription set --priority <true|false>`; `pod subscribe` also accepts a canonical public URL of the form `https://origin.example/federation/pods/<slug>`. Pod governance uses only `owner` and `curator`: `pod role list`, `pod role grant --user-id <id> --role <owner|curator>`, and `pod role revoke ...`. Grants and revocations return Pending Proposals and do not take effect until an independent Owner or scoped interactive approval Harness approves them.

Create Pods with `stumble pod create --name <name> --slug <slug> --visibility <private|invite-only|public>`. Add either `--package <directory>` for a complete initial Pod Package or `--from-pod <slug-or-id>` to derive one with source-package provenance; the two options are mutually exclusive. Public creation and visibility expansion return Pending Proposals, while visibility restrictions apply directly. `stumble pod explore --query <subject>` returns Trust Policy-filtered public Pods and bounded sample Content Items without creating a Subscription.

Read a Pod's complete accepted stream with `stumble pod content list <slug-or-id>` and inspect its Content Item and Accepted Placement evidence with `pod content show <pod> <content-item-id>`. Authorized curators can use `pod content add <pod> <content-item-id> [--note ...]` or `pod content remove <pod> <content-item-id> --reason ...`; private removal is immediate, while public removal returns a Pending Proposal and publishes a Placement Tombstone only after approval. `pod policy show <pod>` reports Manual, Assisted, or Autonomous Curation. `pod policy set <pod> --mode <manual|assisted|autonomous>` applies Manual and Assisted directly, requires `--confidence-threshold` for Assisted and Autonomous, and routes Autonomous enablement through approval.

Inspect the current immutable Pod Package with `stumble pod package show <pod>` or add `--version <number>` for a historical version. `pod package export <pod> --output <directory>` writes the complete portable artifact and signed provenance history; check an edited artifact without changing state with `pod package validate --package <directory>`. Apply it with `pod package revise <pod> --base-version <number> --package <directory>`: stale bases fail, non-public origin packages revise directly, and public revisions wait for Pending Proposal approval.

### Discovery tasks

- `stumble discover task list` — Automatically materializes due work from current Source Rules, then returns the scoped task collection. Use `--state <ready|pending|leased|completed|terminal-failure>`, `--pod <slug-or-id>`, `--limit`, and `--cursor`.
- `stumble discover task show <ID>` — Inspects current state, attempt history, and allowed actions.
- `stumble discover task claim|renew <ID> --lease-seconds <SECONDS>` — Acquires or extends an exclusive Harness-owned lease.
- `stumble discover task complete <ID>` and `stumble discover task fail <ID> --reason <REASON>` — Finish the current owned attempt. Failures remain retryable until the attempt limit is reached.

Manual task creation and materialization are not public commands. Scheduler
Adapters wake workers through the same filtered list surface and never control
a browser.

### Candidate curation

- `stumble discover candidate submit --input <FILE|-> --idempotency-key <KEY>` records structured source metadata, provenance, and proposed Pod Placements. Retrying identical input with the same key returns the original result; changed input conflicts.
- `stumble discover candidate list [--status <pending|accepted>]` returns the scoped, cursor-paginated Candidate collection. `candidate show <ID>` includes submissions, placement evidence and state, and allowed actions.
- `stumble discover candidate evaluate <ID>` applies each target Pod's current Curation Policy independently. Curators can add an evidence-backed local proposal with `candidate route <ID> <POD> --reason <TEXT> --confidence <0..1>` and decide exactly one pending placement with `candidate review <ID> <POD> --decision <accept|reject> [--note <TEXT>]`.

### Feed workflows

- `stumble feed batch get [--input <FILE|->]` returns the current stable Feed Batch. Structured input can set `size`, `recurrence_penalty_days`, `feed_mix`, and temporary `batch_intent`; repeated reads return the same batch until `feed batch complete <ID>` explicitly reaches Caught Up.
- `stumble feed feedback record <CONTENT_ITEM_ID> --kind <save|more-like-this|less-like-this|dismiss|block-source|block-topic>` records a private Feedback Signal for a Delivered Item. Topic blocks also require `--topic <ITEM_TOPIC>`; use `--reason` for optional context.
- `stumble feed taste show` inspects explicit preferences and learned weights. `feed taste set --input <FILE|->` replaces supplied explicit fields, while `feed taste reset [--input <FILE|->]` clears all learned weights or the structured `signal` selection without changing explicit preferences.

Drip remains conversational Agent Harness language rather than a command. Source and topic blocks are item-driven feedback; deliberate bulk blocks belong in `feed taste set` input.

### Synchronization

Trusted peers are local Trust Policy entries. `stumble sync peer add --node-id
<NODE_ID> --display-name <NAME> --base-url <URL> --public-key <KEY>` and
`sync peer remove <PEER_ID>` create Pending Proposals; approval is required
before either trust change takes effect. `sync peer list [--limit N] [--cursor
CURSOR]` returns only enabled peers and their canonical Node identities.

Subscription synchronization normally runs through the Node Agent without a
manual command. For diagnosis or recovery, `stumble sync pod run <POD>
--peer <PEER_ID>` verifies the selected trusted peer against the Subscription's
pinned Origin Node and applies the next signed event segment. `sync pod status
<POD>` reports the cursor, verification state, latest event, last successful
run, and the latest actionable failure. Signed-event file export, import, and
verification are internal protocol tools and are not CLI commands.

Run `stumble --help` or `stumble <family> --help` for accepted workflows and defaults.
