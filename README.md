# Stumble

Stumble is a decentralized personal discovery system. It builds a personal Feed from independently curated Pods on a local Home Node.

## Get started

Install [Rust](https://rustup.rs), clone this repository, then run:

```bash
cargo install --path crates/stumble-cli --locked
stumble node init
stumble add "https://example.com/something-worth-keeping"
stumble feed batch get
```

That is the whole loop: initialize once, add links as you find them, read your Feed when you want it. `stumble add` creates a private `saved` Pod on first use, places the link in it, and makes it Feed-eligible in one step.

Stumble stores its Home Node under `~/.stumble/nodes/home` by default. Set `STUMBLE_DATA_DIR` or pass `--data-dir` to use another directory. `stumble node init` also records the local Owner credential in the operating system's credential store; later local commands detect its presence automatically. Pass `--demo` to `node init` for throwaway fixture data.

### Use it from an AI harness

Stumble is designed to be driven by an agent harness (Claude Code, Codex, Hermes, Pi, ...). The harness owns the browser — it reads pages with your logged-in sessions — and Stumble owns the collection and the Feed. Install the [`SKILL.md`](SKILL.md) at the repository root into your harness's skills directory and it will know the loop: open a shared link, understand it, `stumble add` it, and read your Feed back on request.

## Quick actions

| Command | Description |
| --- | --- |
| `stumble add <url>` | Add a link to a Pod and your Feed in one step (`--pod`, `--title`, `--summary`, `--tag`, `--note`). |

The rest of the CLI is JSON-first and organized into five workflow families. Add `--help` at any command level for arguments and defaults.

## `node`

Manage the Home Node, Agent Harnesses, and approval proposals.

| Command | Description |
| --- | --- |
| `stumble node init` | Initialize the Home Node (`--demo` seeds fixtures). |
| `stumble node show` | Show Home Node identity and status. |
| `stumble node harness list` | List registered Agent Harnesses. |
| `stumble node harness show` | Show one Agent Harness. |
| `stumble node harness register` | Register an Agent Harness. |
| `stumble node harness revoke` | Revoke an Agent Harness. |
| `stumble node proposal list` | List pending approval proposals. |
| `stumble node proposal show` | Show one proposal. |
| `stumble node proposal approve` | Approve a proposal. |
| `stumble node proposal reject` | Reject a proposal. |

## `pod`

Find, subscribe to, curate, and govern Pods.

| Command | Description |
| --- | --- |
| `stumble pod list` | List local Pods. |
| `stumble pod show` | Show one Pod. |
| `stumble pod create` | Create a Pod. |
| `stumble pod explore` | Explore public Pods. |
| `stumble pod subscribe` | Subscribe to a Pod. |
| `stumble pod unsubscribe` | Unsubscribe from a Pod. |
| `stumble pod subscription set` | Set subscription priority. |
| `stumble pod visibility set` | Change Pod visibility. |
| `stumble pod role list` | List Pod roles. |
| `stumble pod role grant` | Grant a Pod role. |
| `stumble pod role revoke` | Revoke a Pod role. |
| `stumble pod content list` | List accepted Pod content. |
| `stumble pod content show` | Show one content item. |
| `stumble pod content add` | Add content to a Pod. |
| `stumble pod content remove` | Remove content from a Pod. |
| `stumble pod policy show` | Show the Pod curation policy. |
| `stumble pod policy set` | Set the Pod curation policy. |
| `stumble pod package show` | Show a Pod Package. |
| `stumble pod package export` | Export a Pod Package. |
| `stumble pod package validate` | Validate a Pod Package directory. |
| `stumble pod package revise` | Revise a Pod Package. |

## `discover`

Run discovery work and curate submitted candidates. Personal Discovery is
User-scoped: the Home Node chooses sources from private evidence so the User
does not need to name platforms.

| Command | Description |
| --- | --- |
| `stumble discover personal readiness` | Check Personal Discovery readiness. |
| `stumble discover personal request` | Request a minimized plan and task. |
| `stumble discover personal plan` | Inspect a Discovery Plan. |
| `stumble discover personal complete-batch` | Complete a result batch for a claimed task. |
| `stumble discover personal batches` | List private result batches. |
| `stumble discover personal batch` | Show one result batch. |
| `stumble discover personal dismiss-batch` | Dismiss a batch without item-level learning. |
| `stumble discover personal review-batch` | Mark a batch reviewed. |
| `stumble discover personal review-item` | Save, place, reinforce, reject, or ignore one item. |
| `stumble discover personal notify-batch` | One-shot results-ready notification attempt. |
| `stumble discover personal schedule create` | Create a named private schedule. |
| `stumble discover personal schedule list` | List schedules and backpressure. |
| `stumble discover personal schedule show` | Inspect one schedule. |
| `stumble discover personal schedule update` | Update schedule configuration. |
| `stumble discover personal schedule disable` | Disable a schedule. |
| `stumble discover personal schedule remove` | Remove a schedule. |
| `stumble discover task list` | List discovery tasks. |
| `stumble discover task show` | Show one discovery task. |
| `stumble discover task claim` | Claim a task lease. |
| `stumble discover task renew` | Renew a task lease. |
| `stumble discover task complete` | Complete a task. |
| `stumble discover task fail` | Record a failed task attempt. |
| `stumble discover candidate list` | List discovery candidates. |
| `stumble discover candidate submit` | Submit candidate input. |
| `stumble discover candidate show` | Show one candidate. |
| `stumble discover candidate evaluate` | Evaluate a candidate against Pod policies. |
| `stumble discover candidate route` | Route a candidate to a Pod. |
| `stumble discover candidate review` | Accept or reject a candidate placement. |

## `feed`

Read Feed batches, record feedback, and manage taste settings.

| Command | Description |
| --- | --- |
| `stumble feed batch get` | Get the current Feed batch. |
| `stumble feed batch complete` | Complete a Feed batch. |
| `stumble feed feedback record` | Record feedback on delivered content. |
| `stumble feed taste show` | Show taste settings and learned weights. |
| `stumble feed taste set` | Set explicit taste preferences. |
| `stumble feed taste reset` | Reset learned taste weights. |
| `stumble feed taste retract` | Retract one private Interest Seed contribution. |

## `sync`

Manage trusted peers and synchronize Pod state.

| Command | Description |
| --- | --- |
| `stumble sync peer list` | List trusted peers. |
| `stumble sync peer add` | Propose adding a trusted peer. |
| `stumble sync peer remove` | Propose removing a trusted peer. |
| `stumble sync pod run` | Synchronize a Pod from a peer. |
| `stumble sync pod status` | Show Pod synchronization status. |
