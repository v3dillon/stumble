# Stumble

<img src="docs/assets/stumble-mascots.png" alt="Stumble mascots" width="300">

Stumble is a decentralized personal discovery system. It builds a personal Feed from independently curated Pods on a local Home Node.

## Get started

```bash
curl -fsSL https://raw.githubusercontent.com/v3dillon/stumble/main/scripts/install.sh | bash
```

That installs three commands into `~/.local/bin` (override with `STUMBLE_INSTALL_DIR`):


| Binary           | Role                                                        |
| ---------------- | ----------------------------------------------------------- |
| `stumble`        | Local CLI — save links, stumble for one item, curate Pods   |
| `stumble-api`    | HTTP server — share Pods, federation, Bootstrap/Index/Relay roles |
| `stumble-runner` | Long-running daemon — network sync, MCP, scheduled workers  |


Then the whole loop:

```bash
stumble node init
stumble add "https://example.com/something-worth-keeping"
stumble
```

Initialize once, add links as you find them, and run `stumble` when you want one new item. Each run of the bare command shows one new item — link, summary, cover — from your Feed. The command starts a fresh batch after it shows every item in the current batch. When your Feed is caught up, the command returns a clearly labeled sample from an unsubscribed public Pod on the network. `stumble add` creates a private `saved` Pod on first use, places the link in it, and makes it Feed-eligible in one step. (`stumble feed batch get` reads the Feed as a batch instead of one item at a time.)

Stumble stores its Home Node under `~/.stumble/nodes/home` by default. Set `STUMBLE_DATA_DIR` or pass `--data-dir` to use another directory. `stumble node init` also records the local Owner credential in the operating system's credential store; later local commands detect its presence automatically. Pass `--demo` to `node init` for throwaway fixture data.

Stumble is designed to be driven by an agent harness (Claude Code, Codex, Hermes, Pi, …). The harness owns source access — it reads sources with its own tools and your existing access — and Stumble owns the collection and the Feed. After the install above, install the skill globally so every harness can find it:

```bash
npx skills add v3dillon/stumble -g -y   # global skill for your agents
npx skills update -g                    # refresh after the repo changes
```

(`-g` installs under each agent’s user skills directory rather than only this checkout; `-y` skips prompts. Without npx, copy the repository’s root [`SKILL.md`](SKILL.md) to `~/.agents/skills/stumble/SKILL.md`.) Your harness then knows the loop: open a shared link, understand it, `stumble add` it, and read your Feed back on request. (Pod skills installed with `stumble pod skill install` update by re-running the install after a `sync pod run`.)

For a guided first run, paste [`llms.txt`](llms.txt) into the harness — or point it at `https://your-bootstrap/llms.txt`, which every node serves with its own URL pre-filled. That script installs Stumble, connects to a Bootstrap, learns your taste from “send me something cool,” runs your first discovery run, and offers your first Pod.

### Share a Pod with a friend

A Pod travels with its context: subscribing pulls the accepted content *and* the Pod Package (`CONTEXT.md` + `SKILL.md`), so your friend's harness immediately understands the Pod's subject and curation rules. `stumble-api` is already on your `PATH` from Get started — keep it running while friends sync.

```bash
# You: publish and serve
stumble pod publish rust-craft --base-url https://your-node.example
stumble-api --bind 0.0.0.0:8787          # keep running while friends sync
                                         # (or stumble-runner --config ~/.config/stumble/runner.yaml serve,
                                         #  which also serves MCP)

# You, with no public address: publish through a Relay instead — nothing to serve.
# Prints a share URL of the shape https://relay.example/relay/pods/<origin-node-id>/rust-craft
stumble pod publish rust-craft --base-url https://relay.example --via-relay

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

Pods can vouch for each other: `stumble pod endorse <slug> --from <your-pod> --reason "..."` signs a recommendation that travels through Bootstrap nodes and shows up as inspectable evidence in other users' local ranking — never as global reputation. Ranking happens entirely on your node — queries and taste never leave it. To help the network, run a Bootstrap, Index, or Relay role — independent flags on one process: `stumble-api --bootstrap` (open announcement admission + streams), `stumble-api --index` (public search over admitted announcements), or `stumble-api --relay` (serves Origin-signed Pod snapshots for private Home Nodes that publish with `stumble pod publish <slug> --base-url <relay> --via-relay`). Deploying one on a VPS is one script — see [docs/deploy-bootstrap.md](docs/deploy-bootstrap.md):

```bash
sudo ./scripts/deploy-bootstrap-vps.sh bootstrap.example.com
```

Point your own node at an Index with `stumble sync discovery index add --label <name> --base-url <url>` — explore then fans explicit queries out to it and still ranks everything locally.

### The morning brief

Stumble plans, your harness finds, you wake up to a shortlist. A daily Personal Discovery schedule (`stumble discover personal schedule create`) materializes a discovery task each morning from your private taste evidence; the harness claims it, reads your sources with its own tools, submits what fits, and presents the batch alongside your Feed as a conversational brief. Nothing enters your Feed or Pods until you say so — see the "Autonomous discovery" and "morning brief" workflows in [`SKILL.md`](SKILL.md).

## Quick actions


| Command                  | Description                                                                                                                                          |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `stumble`                | One new item from your Feed, or from the network when caught up. Prints a text card; `--format json` for harnesses.                                  |
| `stumble add <url>`      | Add a link to a Pod and your Feed in one step (`--pod`, `--title`, `--summary`, `--excerpt`, `--tag`, `--note`, `--image`, `--cover`, `--snapshot`). |
| `stumble search <query>` | Local BM25 full-text search over everything saved on this node — titles, summaries, tags, notes, snapshots (`--limit`, 1-50, default 10).            |
| `stumble context show`   | Show the private briefing packet: your User Context prose, taste, watches, readiness (`context set --input` replaces the prose).                     |
| `stumble brief get`      | Compose the morning brief in one call: `outside`, `network.feed`, `network.explore`, `gaps`. The node fills every section.                           |


The rest of the CLI is JSON-first and organized into five workflow families, plus top-level `context` and `brief`. Add `--help` at any command level for arguments and defaults.

## `context`

Load the User before you save, discover, or write a brief.


| Command                     | Description                                                                 |
| --------------------------- | --------------------------------------------------------------------------- |
| `stumble context show`      | Show the private briefing packet: User Context, taste, watches, readiness.  |
| `stumble context set`       | Replace the User Context prose (`--input` JSON with `context_md`).          |


## `brief`

Compose one morning brief. The node fills every section.


| Command              | Description                                                                              |
| -------------------- | ---------------------------------------------------------------------------------------- |
| `stumble brief get`  | Return `user`, `outside` (Discovery Result Batch), `network` (Feed + Explore), and `gaps`. |


## `node`

Manage the Home Node, Agent Harnesses, and approval proposals.


| Command                         | Description                                         |
| ------------------------------- | --------------------------------------------------- |
| `stumble node init`             | Initialize the Home Node (`--demo` seeds fixtures). |
| `stumble node show`             | Show Home Node identity and status.                 |
| `stumble node harness list`     | List registered Agent Harnesses.                    |
| `stumble node harness show`     | Show one Agent Harness.                             |
| `stumble node harness register` | Register an Agent Harness.                          |
| `stumble node harness revoke`   | Revoke an Agent Harness.                            |
| `stumble node proposal list`    | List pending approval proposals.                    |
| `stumble node proposal show`    | Show one proposal.                                  |
| `stumble node proposal approve` | Approve a proposal.                                 |
| `stumble node proposal reject`  | Reject a proposal.                                  |


## `pod`

Find, subscribe to, curate, and govern Pods.


| Command                        | Description                                                                     |
| ------------------------------ | ------------------------------------------------------------------------------- |
| `stumble pod list`             | List local Pods.                                                                |
| `stumble pod show`             | Show one Pod.                                                                   |
| `stumble pod create`           | Create a Pod.                                                                   |
| `stumble pod delete`           | Delete a Pod that you own. The Inbox cannot be deleted.                         |
| `stumble pod explore`          | Explore public Pods.                                                            |
| `stumble pod publish`          | Make a Pod public and print its shareable URL.                                  |
| `stumble pod endorse`          | Sign a recommendation of another public Pod (`--from`, `--reason`).             |
| `stumble pod announce`         | Re-sign announcements (lease renewal + latest content) and push to Bootstraps.  |
| `stumble pod subscribe`        | Subscribe to a local Pod by slug or a public Pod by URL.                        |
| `stumble pod unsubscribe`      | Unsubscribe from a Pod.                                                         |
| `stumble pod subscription set` | Set subscription priority.                                                      |
| `stumble pod visibility set`   | Change Pod visibility.                                                          |
| `stumble pod role list`        | List Pod roles.                                                                 |
| `stumble pod role grant`       | Grant a Pod role.                                                               |
| `stumble pod role revoke`      | Revoke a Pod role.                                                              |
| `stumble pod content list`     | List accepted Pod content.                                                      |
| `stumble pod content show`     | Show one content item.                                                          |
| `stumble pod content add`      | Add content to a Pod.                                                           |
| `stumble pod content remove`   | Remove content from a Pod.                                                      |
| `stumble pod content cover`    | Store a local image as an item's cover (`--file`, `--source`, `--alt`).         |
| `stumble pod content snapshot` | Archive a reader-mode text copy of an item's page (`--file`, `--source`).       |
| `stumble pod policy show`      | Show the Pod curation policy.                                                   |
| `stumble pod policy set`       | Set the Pod curation policy.                                                    |
| `stumble pod package show`     | Show a Pod Package.                                                             |
| `stumble pod package export`   | Export a Pod Package.                                                           |
| `stumble pod package validate` | Validate a Pod Package directory.                                               |
| `stumble pod package revise`   | Revise a Pod Package.                                                           |
| `stumble pod skill install`    | Install a Pod's SKILL.md into a harness skills directory (`--dir`; owner-only). |


## `discover`

Run discovery work and curate submitted candidates. Personal Discovery is
User-scoped: the Home Node chooses sources from private evidence so the User
does not need to name platforms.


| Command                                      | Description                                         |
| -------------------------------------------- | --------------------------------------------------- |
| `stumble discover personal readiness`        | Check Personal Discovery readiness.                 |
| `stumble discover personal request`          | Request a minimized plan and task.                  |
| `stumble discover personal plan`             | Inspect a Discovery Plan.                           |
| `stumble discover personal complete-batch`   | Complete a result batch for a claimed task.         |
| `stumble discover personal batches`          | List private result batches.                        |
| `stumble discover personal batch`            | Show one result batch.                              |
| `stumble discover personal dismiss-batch`    | Dismiss a batch without item-level learning.        |
| `stumble discover personal review-batch`     | Mark a batch reviewed.                              |
| `stumble discover personal review-item`      | Save, place, reinforce, reject, or ignore one item. |
| `stumble discover personal notify-batch`     | One-shot results-ready notification attempt.        |
| `stumble discover personal schedule create`  | Create a named private schedule.                    |
| `stumble discover personal schedule list`    | List schedules and backpressure.                    |
| `stumble discover personal schedule show`    | Inspect one schedule.                               |
| `stumble discover personal schedule update`  | Update schedule configuration.                      |
| `stumble discover personal schedule disable` | Disable a schedule.                                 |
| `stumble discover personal schedule remove`  | Remove a schedule.                                  |
| `stumble discover watch add <url>`           | Add a User-scoped watch (`--kind timeline\|account\|site`, `--cadence`, `--skill`). |
| `stumble discover watch list`                | List watches with last availability.                |
| `stumble discover watch remove <id>`         | Remove a User-scoped watch.                         |
| `stumble discover task list`                 | List discovery tasks.                               |
| `stumble discover task show`                 | Show one discovery task.                            |
| `stumble discover task claim`                | Claim a task lease.                                 |
| `stumble discover task renew`                | Renew a task lease.                                 |
| `stumble discover task complete`             | Complete a task.                                    |
| `stumble discover task fail`                 | Record a failed task attempt.                       |
| `stumble discover candidate list`            | List discovery candidates.                          |
| `stumble discover candidate submit`          | Submit candidate input.                             |
| `stumble discover candidate show`            | Show one candidate.                                 |
| `stumble discover candidate evaluate`        | Evaluate a candidate against Pod policies.          |
| `stumble discover candidate route`           | Route a candidate to a Pod.                         |
| `stumble discover candidate review`          | Accept or reject a candidate placement.             |


## `feed`

Read Feed batches, record feedback, and manage taste settings.


| Command                        | Description                                     |
| ------------------------------ | ----------------------------------------------- |
| `stumble feed batch get`       | Get the current Feed batch.                     |
| `stumble feed batch complete`  | Complete a Feed batch.                          |
| `stumble feed feedback record` | Record feedback on delivered content.           |
| `stumble feed taste show`      | Show taste settings and learned weights.        |
| `stumble feed taste set`       | Set explicit taste preferences.                 |
| `stumble feed taste reset`     | Reset learned taste weights.                    |
| `stumble feed taste retract`   | Retract one private Interest Seed contribution. |


## `sync`

Manage trusted peers and synchronize Pod state.


| Command                                | Description                                                        |
| -------------------------------------- | ------------------------------------------------------------------ |
| `stumble sync peer list`               | List trusted peers.                                                |
| `stumble sync peer add`                | Propose adding a trusted peer.                                     |
| `stumble sync peer remove`             | Propose removing a trusted peer.                                   |
| `stumble sync pod run`                 | Synchronize a Pod from a peer.                                     |
| `stumble sync pod status`              | Show Pod synchronization status.                                   |
| `stumble sync bootstrap list`          | List configured Bootstrap endpoints.                               |
| `stumble sync bootstrap status`        | Report Bootstrap endpoints with cursor and failure state.          |
| `stumble sync bootstrap run`           | Synchronize Announcement Streams from enabled Bootstrap endpoints. |
| `stumble sync bootstrap add`           | Add a replaceable Bootstrap endpoint.                              |
| `stumble sync bootstrap enable`        | Re-enable a Bootstrap endpoint.                                    |
| `stumble sync bootstrap disable`       | Disable a Bootstrap endpoint.                                      |
| `stumble sync bootstrap remove`        | Remove a Bootstrap endpoint.                                       |
| `stumble sync discovery status`        | Report discovery readiness, including degraded mode.               |
| `stumble sync discovery serve show`    | Show the inbound Discovery Peer serving state.                     |
| `stumble sync discovery serve enable`  | Enable inbound announcement serving.                               |
| `stumble sync discovery serve disable` | Disable inbound announcement serving.                              |
| `stumble sync discovery peers`         | List the rotating outbound Discovery Peer set.                     |
| `stumble sync discovery gossip`        | Enable or disable automatic peer gossip.                           |
| `stumble sync discovery run`           | Learn Discovery Peers and synchronize their streams.               |
| `stumble sync discovery index list`    | List configured Index Nodes.                                       |
| `stumble sync discovery index add`     | Add a replaceable Index Node (`--label`, `--base-url`).            |
| `stumble sync discovery index remove`  | Remove an Index Node.                                              |


## Develop

Day-to-day product use is Get started (prebuilt binaries). This section is for contributors.

### Build from source

Install [Rust](https://rustup.rs), clone this repository, then:

```bash
git clone https://github.com/v3dillon/stumble && cd stumble
cargo install --path crates/stumble-cli --locked
```

That installs the same three binaries (`stumble`, `stumble-api`, `stumble-runner`) as the release tarball. Optional MCP server:

```bash
cargo install --path crates/stumble-mcp --locked
```

### Multi-crate layout

The repository is multi-crate so each surface stays focused; users still get one product install:


| Crate          | What it is                                                                                        |
| -------------- | ------------------------------------------------------------------------------------------------- |
| `stumble-cli`  | **Install package** — ships `stumble`, `stumble-api`, and `stumble-runner`                        |
| `stumble-core` | Domain model, store, agent tools (library)                                                        |
| `stumble-api`  | Node-to-node HTTP surface (library; binary entrypoint lives here, binary target in `stumble-cli`) |
| `stumble-mcp`  | MCP transport (optional separate binary)                                                          |
| `stumble-sync` | Sync helpers (library)                                                                            |


```bash
cargo test -p stumble-core
cargo test -p stumble-api
cargo test -p stumble-cli
cargo build -p stumble-cli --release --bin stumble-api
```

### Publishing a release

Push a version tag; CI builds platform tarballs and attaches them to a GitHub Release (plus `install.sh` and `SHA256SUMS`):

```bash
git tag v0.1.0
git push origin v0.1.0
```

Targets: `macos-arm64`, `macos-x86_64`, `linux-arm64`, `linux-x86_64`. Workflow: [`.github/workflows/release.yml`](.github/workflows/release.yml).

`stumble` has no `serve` or `--api` mode on purpose (local workflows stay local). Remote reachability is always the separate `stumble-api` process — it just ships in the same install.