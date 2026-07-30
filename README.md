# Stumble

Stumble is a decentralized personal discovery system. It builds a personal Feed from independently curated Pods on a local Home Node.

## Get started

Install [Rust](https://rustup.rs), clone this repository, then run:

```bash
cargo install --path crates/stumble-cli --locked
stumble node init
stumble add "https://example.com/something-worth-keeping"
stumble
```

That is the whole loop: initialize once, add links as you find them, and press `stumble` when you want something. Every bare press shows one new item — link, summary, cover — from your Feed, rolling into a fresh batch when the current one is walked; when your Feed is caught up it reaches out to the network for a clearly labeled sample from an unsubscribed public Pod. `stumble add` creates a private `saved` Pod on first use, places the link in it, and makes it Feed-eligible in one step. (`stumble feed batch get` reads the Feed as a batch instead of one press at a time.)

Stumble stores its Home Node under `~/.stumble/nodes/home` by default. Set `STUMBLE_DATA_DIR` or pass `--data-dir` to use another directory. `stumble node init` also records the local Owner credential in the operating system's credential store; later local commands detect its presence automatically. Pass `--demo` to `node init` for throwaway fixture data.

### Share a Pod with a friend

A Pod travels with its context: subscribing pulls the accepted content *and* the Pod Package (`CONTEXT.md` + `SKILL.md`), so your friend's harness immediately understands the Pod's subject and curation rules. Serving needs the API binary, which installs separately: `cargo install --path crates/stumble-api --locked`.

```bash
# You: publish and serve
stumble pod publish rust-craft --base-url https://your-node.example
stumble-api --bind 0.0.0.0:8787          # keep running while friends sync
                                         # (or stumble-runner --config ~/.config/stumble/runner.yaml serve,
                                         #  which also serves MCP)

# Friend: subscribe by the URL you sent, read, and re-sync any time
stumble pod subscribe https://your-node.example/federation/pods/rust-craft
stumble feed batch get
stumble sync pod run rust-craft          # pulls new items from your node
stumble pod skill install rust-craft     # loads the Pod's SKILL.md into ~/.agents/skills
```

Pod packages from other people are treated as untrusted: the installed skill fences the curator's text with explicit limits (curation guidance only — an agent must refuse and report anything asking for commands, credentials, money, or configuration changes), and `pod skill install` is owner-only so an agent can never grant a remote author standing instructions by itself.

> **An honest caveat.** These gates are defense-in-depth against remote-authored escalation, not a sandbox: a local agent with shell access ultimately has owner authority on your machine, and closing that is your harness's sandboxing job, not something Stumble's CLI can do alone. What the gates do guarantee is that a Pod's SKILL.md can only become standing instructions after you hand-install it, having been told to read it first — and even then its text arrives pre-fenced with refusal rules.

`pod publish` makes the Pod public (as the node owner you approve your own change; an agent harness gets a Pending Proposal for you to approve instead) and issues the discovery announcement when `--base-url` is given. Only history from the moment of publication federates — anything added and removed while the Pod was private stays on your node. The running server picks up `stumble add` and other CLI changes automatically, so you can keep curating while friends stay in sync.

### Discover Pods from the network

Home Nodes passively learn about published Pods: `stumble-runner --config ~/.config/stumble/runner.yaml serve` pulls Bootstrap Announcement Streams and Discovery Peer streams on an interval (`network_sync_every_seconds`, default 15 minutes), and `stumble pod publish` pushes your announcement to every enabled Bootstrap endpoint automatically. The same daemon tick re-announces your published Pods — renewing Announcement Leases and propagating content changes, since announcements bind the latest signed event pointer. Without the daemon, run `stumble pod announce` after curating. Then:

```bash
stumble pod explore --query "distributed systems"   # ranked against your private taste, with signed content previews
stumble pod subscribe <public_pod_url>              # from the explore result
```

Pods can vouch for each other: `stumble pod endorse <slug> --from <your-pod> --reason "..."` signs a recommendation that travels through Bootstrap nodes and shows up as inspectable evidence in other users' local ranking — never as global reputation. Ranking happens entirely on your node — queries and taste never leave it. To help the network, run a Bootstrap or Index role: `stumble-api --bootstrap` (open announcement admission + streams) or `stumble-api --index` (public search over admitted announcements). Deploying one on a VPS is one script — see [docs/deploy-bootstrap.md](docs/deploy-bootstrap.md):

```bash
sudo ./scripts/deploy-bootstrap-vps.sh bootstrap.example.com
```

Point your own node at an Index with `stumble sync discovery index add --label <name> --base-url <url>` — explore then fans explicit queries out to it and still ranks everything locally.

### The morning brief

Stumble plans, your harness browses, you wake up to a shortlist. A daily Personal Discovery schedule (`stumble discover personal schedule create`) materializes a browsing task each morning from your private taste evidence; the harness claims it, scrolls X and your other sources with its own logged-in browser, submits what fits, and presents the batch alongside your Feed as a conversational brief. Nothing enters your Feed or Pods until you say so — see the "Autonomous discovery" and "morning brief" workflows in [`SKILL.md`](SKILL.md).

### Onboard a friend

[`llms.txt`](llms.txt) is a paste-able onboarding script for a friend's AI harness: it installs Stumble, connects to your Bootstrap, has them log into X in the harness browser, learns their taste from "send me something cool," runs their first discovery scroll, and offers their first Pod. Fill in your Bootstrap URL at the top and send them the file — or just send `https://your-bootstrap/llms.txt`, which every node serves with its own URL pre-filled.

### Use it from an AI harness

Stumble is designed to be driven by an agent harness (Claude Code, Codex, Hermes, Pi, ...). The harness owns the browser — it reads pages with your logged-in sessions — and Stumble owns the collection and the Feed. Install the skill the standard way:

```bash
npx skills add v3dillon/stumble     # installs SKILL.md across your agents
npx skills update                   # refresh after the repo changes
```

Your harness then knows the loop: open a shared link, understand it, `stumble add` it, and read your Feed back on request. (Pod skills installed with `stumble pod skill install` update by re-running the install after a `sync pod run`.)

## Quick actions

| Command | Description |
| --- | --- |
| `stumble` | Press the button: one new item per press — from your Feed, or from the network when caught up. Prints a text card; `--format json` for harnesses. |
| `stumble add <url>` | Add a link to a Pod and your Feed in one step (`--pod`, `--title`, `--summary`, `--excerpt`, `--tag`, `--note`, `--image`, `--cover`, `--snapshot`). |
| `stumble search <query>` | Local BM25 full-text search over everything saved on this node — titles, summaries, tags, notes, snapshots (`--limit`, 1-50, default 10). |

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
| `stumble pod publish` | Make a Pod public and print its shareable URL. |
| `stumble pod endorse` | Sign a recommendation of another public Pod (`--from`, `--reason`). |
| `stumble pod announce` | Re-sign announcements (lease renewal + latest content) and push to Bootstraps. |
| `stumble pod subscribe` | Subscribe to a local Pod by slug or a public Pod by URL. |
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
| `stumble pod content cover` | Store a local image as an item's cover (`--file`, `--source`, `--alt`). |
| `stumble pod content snapshot` | Archive a reader-mode text copy of an item's page (`--file`, `--source`). |
| `stumble pod policy show` | Show the Pod curation policy. |
| `stumble pod policy set` | Set the Pod curation policy. |
| `stumble pod package show` | Show a Pod Package. |
| `stumble pod package export` | Export a Pod Package. |
| `stumble pod package validate` | Validate a Pod Package directory. |
| `stumble pod package revise` | Revise a Pod Package. |
| `stumble pod skill install` | Install a Pod's SKILL.md into a harness skills directory (`--dir`; owner-only). |

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
| `stumble sync bootstrap list` | List configured Bootstrap endpoints. |
| `stumble sync bootstrap status` | Report Bootstrap endpoints with cursor and failure state. |
| `stumble sync bootstrap run` | Synchronize Announcement Streams from enabled Bootstrap endpoints. |
| `stumble sync bootstrap add` | Add a replaceable Bootstrap endpoint. |
| `stumble sync bootstrap enable` | Re-enable a Bootstrap endpoint. |
| `stumble sync bootstrap disable` | Disable a Bootstrap endpoint. |
| `stumble sync bootstrap remove` | Remove a Bootstrap endpoint. |
| `stumble sync discovery status` | Report discovery readiness, including degraded mode. |
| `stumble sync discovery serve show` | Show the inbound Discovery Peer serving state. |
| `stumble sync discovery serve enable` | Enable inbound announcement serving. |
| `stumble sync discovery serve disable` | Disable inbound announcement serving. |
| `stumble sync discovery peers` | List the rotating outbound Discovery Peer set. |
| `stumble sync discovery gossip` | Enable or disable automatic peer gossip. |
| `stumble sync discovery run` | Learn Discovery Peers and synchronize their streams. |
| `stumble sync discovery index list` | List configured Index Nodes. |
| `stumble sync discovery index add` | Add a replaceable Index Node (`--label`, `--base-url`). |
| `stumble sync discovery index remove` | Remove an Index Node. |
