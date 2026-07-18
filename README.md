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

To start the optional local HTTP API, run `podctl --data-dir ~/.stumble/nodes/default serve`.

## Legacy CLI

The pre-release `podctl` executable remains temporarily as an expand-phase bridge while repository callers move to `stumble`. It currently has **46 public commands**; another 10 hidden commands preserve retired pre-release contracts. Every command supports `-h, --help`, while the global flags are `--api <URL>`, `--token <TOKEN>`, and `--data-dir <PATH>`.

### Node

- `init-node` — Initializes the Home Node database. Flags: none.
- `mcp` — Serves authenticated MCP messages over standard input and output. Flags: none.
- `serve` — Starts the Home Node HTTP API. Flags: `--mode <local|hosted>`, `--bind <ADDRESS>`, `--port <PORT>`, and `--allow-public-dev-tokens`.
- `node-info` — Prints node information as JSON. Flags: none.

### Pods and packages

- `create-pod` — Creates a Pod. Flags: `--name`, `--slug`, and optional `--description`.
- `create-pod-package` — Creates a Pod and imports its Pod Package from a directory. Flags: `--name`, `--slug`, `--from`, and optional `--description`.
- `list-pods` — Lists Pods available to the current User or Agent Harness. Flags: none.
- `join-pod <POD>` — Creates a Subscription to a Pod. Flags: none.
- `priority-subscription <POD_ID> <true|false>` — Enables or disables a Priority Subscription. Flags: none.
- `get-pod-package <POD>` — Prints a Pod Package as JSON. Flags: none.
- `export-pod-package <POD> <OUT>` — Exports a Pod Package to a directory. Flags: none.
- `import-pod-package <POD> <FROM>` — Imports a Pod Package directory into a Pod. Flags: none.
- `fork-pod-package` — Forks a Pod Package into a new Pod. Flags: `--source-pod`, `--name`, and `--slug`.
- `validate-pod-package <POD>` — Validates a Pod Package. Flags: none.

### Candidates, feeds, and taste

- `submit-candidate` — Submits a Candidate from a JSON file. Flags: `--from <FILE>`.
- `inspect-candidate <ID>` — Prints a Candidate and its review state. Flags: none.
- `feed` — Creates a finite Feed Batch. Flags: `--size`, `--recurrence-penalty-days`, `--high-value-percent`, `--exploration-percent`, `--old-gem-percent`, `--per-pod-cap`, `--per-source-cap`, `--focus`, and `--avoid`.
- `complete-feed <ID>` — Marks a Feed Batch complete. Flags: none.
- `feed-feedback <CONTENT_ITEM_ID>` — Records a Feedback Signal for a Content Item. Flags: `--kind`, optional `--topic`, and optional `--reason`.
- `taste-profile` — Prints the local Taste Profile. Flags: none.
- `update-taste-profile` — Updates explicit Taste Profile settings. Flags: `--interests`, `--blocked-topics`, `--blocked-sources`, and `--recurrence-penalty-days`.
- `reset-learned-taste` — Clears learned Taste Profile weights globally or for one topic or source. Flags: mutually exclusive `--topic` and `--source`.

### Harnesses and approvals

- `register-harness` — Registers an Agent Harness and returns its token once. Flags: `--label`, `--kind <interactive|unattended>`, repeatable `--capability`, and repeatable `--pod-id`.
- `revoke-harness <ID>` — Revokes a Harness Grant. Flags: none.
- `propose-change` — Creates a Pending Proposal from a JSON file. Flags: `--from <FILE>` and `--expires-in-seconds`.
- `get-proposal <ID>` — Prints a Pending Proposal. Flags: none.
- `approve-proposal <ID>` — Approves a Pending Proposal. Flags: none.
- `reject-proposal <ID>` — Rejects a Pending Proposal. Flags: `--reason`.

### Discovery tasks

- `materialize-discovery-tasks` — Creates due Discovery Tasks from Pod schedules. Flags: none.
- `list-discovery-tasks` — Lists all visible Discovery Tasks. Flags: none.
- `list-ready-discovery-tasks` — Lists Discovery Tasks ready to claim. Flags: none.
- `create-discovery-task <POD_ID>` — Creates a Discovery Task for a Pod. Flags: `--instructions` and `--idempotency-key`.
- `discovery-task-status <ID>` — Prints a Discovery Task's status. Flags: none.
- `claim-discovery-task <ID>` — Claims a task for a limited lease. Flags: `--lease-seconds`.
- `renew-discovery-task <ID>` — Renews a claimed task's lease. Flags: `--lease-seconds`.
- `complete-discovery-task <ID>` — Marks a claimed task complete. Flags: none.
- `fail-discovery-task <ID>` — Marks a claimed task failed. Flags: `--reason`.

### Administration and synchronization

- `create-tenant <SLUG> <NAME>` — Creates a tenant. Flags: none.
- `create-api-token` — Creates an API token. Flags: optional `--user`, optional `--tenant`, and optional `--label`.
- `list-api-tokens` — Lists API tokens. Flags: none.
- `add-peer` — Adds a trusted remote peer. Flags: `--display-name`, `--base-url`, and `--public-key`.
- `list-peers` — Lists configured peers. Flags: none.
- `sync-pod <POD> <PEER_ID>` — Synchronizes a subscribed Pod from a peer. Flags: none.
- `export-events <POD>` — Prints a Pod's signed Pod Events. Flags: none.
- `import-events <POD> <PEER_ID> <FILE>` — Imports signed Pod Events from a file. Flags: none.
- `verify-events <POD>` — Verifies a Pod's signed event history. Flags: none.

Run `podctl <command> --help` for accepted values and defaults.
