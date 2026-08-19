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
target/release/stumble --data-dir ~/.stumble/nodes/home node init
target/release/stumble --data-dir ~/.stumble/nodes/origin node init
target/release/stumble-api --data-dir ~/.stumble/nodes/origin \
  --bind 127.0.0.1:8788
```

`stumble.sqlite3` is authoritative. Back up the complete node directory while
the node is stopped. Restore that directory as a unit and restart with the same
`--data-dir`; signed identity, package history, synchronization cursors, tasks,
Feed history, and private projections then resume together. `stumble node init` creates
an empty Home Node by default; pass `--demo` only when you want seed fixtures.

## Harness grants and tools

Create the private Pod and complete portable Package with an interactive harness
that has `pod-curation` and `package-management`. Then register an unattended
worker restricted to its Pod and discovery capabilities:

```bash
target/release/stumble --data-dir ~/.stumble/nodes/home node harness register \
  --label "Nightly resilient-systems discovery" --kind unattended \
  --capability discovery_tasks --capability candidate_submission \
  --pod-id <pod-id>
```

Capture the plaintext credential when issued; it is shown once. Keep interactive
approval, feedback, Subscription management, and administration on a separate
grant. Revoke a lost or retired harness immediately with
`stumble node harness revoke <harness-id>`; the revocation is effective without
a restart.

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

HTTP, MCP, and `stumble` expose the same domain contracts. Do not query or edit
SQLite to complete any workflow.

MCP clients reach the node through the unified runner. Create its config once
per node and declare one `mcp` profile per Harness Grant; each profile names a
credential command that prints one scoped Harness token, so tokens live in the
Keychain (or another store), never in the YAML or a client config:

```bash
mkdir -p "$HOME/.config/stumble"
cp config/runner.example.yaml "$HOME/.config/stumble/runner.yaml"
# Edit data_dir, credential commands, and the `mcp:` profile map.
```

For a local MCP client on the same machine, configure it to launch one stdio
process per profile:

```bash
stumble-runner --config "$HOME/.config/stumble/runner.yaml" mcp manager
```

The process resolves that profile's Harness token through its credential
command, reads and writes one JSON-RPC message per line, and reserves standard
error for diagnostics. Invalid and revoked Harness tokens fail before protocol
output. No port, background job, HTTPS, or OAuth is needed for stdio.

HTTP clients use the same runner's single loopback listener,
`stumble-runner ... serve`, which binds `127.0.0.1:8790` by default and serves
every configured profile at `/mcp`. Every request carries the profile's
Harness token as a bearer token; unexpected browser `Origin` headers are
rejected. Give an unattended harness an isolated MCP config containing only
`http://127.0.0.1:8790/mcp` and its worker token.

(The standalone `stumble-mcp` binary still exists for manual, advanced use —
stdio with `STUMBLE_MCP_TOKEN`, or its own HTTP bridge — but the launchd
installer removes any installed copy as a legacy artifact; prefer the runner.)

For a two-node federation workflow, run one runner config per node and keep
two named profile families, `stumble-origin` and `stumble-subscriber`, rather
than sharing one profile or credential across nodes. Because each profile is
bound to one fixed Harness token, declare a grant-specific profile for every
independent authority used at the same node — for example
`stumble-origin-curator`, `stumble-origin-approver`,
`stumble-origin-discovery`, and `stumble-origin-reader` against the Origin
Node's config, plus `stumble-subscriber-manager` and
`stumble-subscriber-reader` against the Home Node's config. A stdio client
then addresses each grant by profile:

```json
{
  "mcpServers": {
    "stumble-origin-curator": {
      "command": "/absolute/path/to/stumble-runner",
      "args": ["--config", "/absolute/path/to/origin-runner.yaml", "mcp", "stumble-origin-curator"]
    },
    "stumble-subscriber-manager": {
      "command": "/absolute/path/to/stumble-runner",
      "args": ["--config", "/absolute/path/to/home-runner.yaml", "mcp", "stumble-subscriber-manager"]
    }
  }
}
```

Use a profile in the `stumble-origin-*` family for public Pod proposals,
Candidate submission, routing, Placement review, and Origin content reads. Send
proposal inspection and approval only to `stumble-origin-approver`, backed by an
independent Approval grant; give discovery workers only Candidate Submission
and Discovery Tasks, and scope curators and readers to the public Pod. Use
`stumble-subscriber-manager` for `subscribe_public_pod` and
`synchronize_subscription`, and `stumble-subscriber-reader` for synchronized
content reads. The subscriber calls the public Pod URL outbound; never copy an
Origin token, Harness Grant, Candidate, or Discovery Task to the Home Node.

## Personal Discovery skill (Agent Harness)

Personal Discovery is User-scoped work. Do **not** ask the User to name
platforms such as X or Hacker News. The Home Node chooses source neighborhoods
from private Interest Seeds, Source Affinities, explicit Taste Profile settings,
and locally matched public Discovery Leads.

Register **two distinct Harness Grants** against the same Home Node:

1. **Interactive management** (`interactive` kind):
   `personal_discovery_management`, `feedback`, and usually
   `candidate_submission` for User URL intake. The same grant exposes
   `get_user_context`, `set_user_context`, `add_user_watch`,
   `list_user_watches`, `remove_user_watch`, and `get_brief`.
2. **Unattended execution** (`unattended` kind):
   `personal_discovery_execution` only. The worker may claim tasks, read only
   its pinned Discovery Plan, submit provenance-bearing Candidates, report
   source availability, and complete a result batch. It must not read the full
   Taste Profile, edit schedules, or broaden Browser Grants.

### Generic interest-based discovery

When the User says something like “find me something interesting”:

1. Call `personal_discovery_readiness` (or `discover personal readiness`).
2. If not ready, help the User submit a few URLs with learning enabled, set an
   explicit interest, or supply temporary topic intent — still without asking
   them to name platforms.
3. Call `request_personal_discovery` with a retry-safe `idempotency_key` and
   optional finite `result_count` (default 10). Do not select a Pod or source.
4. With the worker grant: `list_ready_discovery_tasks` → `claim_discovery_task`
   → `get_discovery_plan` (only the minimized plan).
5. Work planned source neighborhoods with this harness's own tools (official
   APIs or plugins, browser sessions, or search). Inspect broadly; submit only
   a finite shortlist of provenance-bearing Candidates bound to the task and
   allocation role (`proven` / `adjacent`).
6. Report availability facts without credentials via
   `report_discovery_source_availability`.
7. Complete with `complete_discovery_result_batch` (this also completes the
   Personal Discovery Task). Present the ready batch to the User with
   provenance, allocation evidence, and inspectable shortfalls.

### User-assisted login

Authentication stays outside Stumble. On-demand runs may surface at most one
authentication-needed notice while continuing accessible planned work.
Scheduled runs never wait for login: they skip unavailable authenticated
sources, reallocate within plan policy, and finish with inspectable reasons.

### Result presentation and explicit feedback

Present the Discovery Result Batch as structured results. Offer deliberate
actions only: Save, Add to Pod, More like this, Not for me, Ignore, or dismiss
the batch. Silence and ignored items create **no** learning. Agent-found
Candidates never train the Taste Profile by themselves. Explicit feedback
changes later plans; review is independent of notification delivery.

### Schedules and scheduled fallback

Named private schedules configure cadence, optional temporary focus/avoidance,
batch size, and delivery mode (`notify_when_supported` or `queue_only`). Each
schedule allows only one unreviewed result batch (backpressure). On-demand
discovery remains available under schedule backpressure.

Use the unified runner when an Agent Harness does not provide its own
scheduler. One loopback process serves bearer-authenticated MCP requests and
runs the configured discovery and curation schedules:

```bash
# In the runner.yaml created under "Harness grants and tools", also edit the
# worker prompts, agent commands, and schedule blocks.
# Copy/edit worker.example.sb (and claude-worker.example.sb for Claude) inside
# each configured working_directory, replacing WORKER and /Users/you.
# Copy the matching codex-, claude-, or grok-worker example into that worker's
# isolated HOME. Rename the MCP server for the profile; for Grok only, place the
# narrow worker token in the isolated config because its HTTP client does not
# expand the token environment variable.
stumble-runner --config "$HOME/.config/stumble/runner.yaml" serve
```

On macOS, install that one daemon with
`scripts/install-runner-launchd.sh`. It installs only `stumble-runner`; no
`libexec` wrappers or separate scheduler jobs are created.

The YAML config names credential commands, MCP profiles, agent commands, and
scheduled workflows. Agent commands are generic argv templates containing
`{prompt}` and optionally `{working_directory}`; Codex, Claude, Grok, and other
CLI harnesses use the same interface. Credentials remain separate by profile,
and HTTP clients must send their assigned Harness token as a bearer token.
Never expose the manager profile to an unattended process. `agent_env` supports `{credential}` and
`{bearer_credential}` placeholders without placing tokens in runner YAML.
The runner writes mode-restricted Discovery-ready events, materializes the same
idempotent task identities on retries, and exits scheduled work silently when
nothing is due. After a scheduled completion, attempt results-ready
notification at most once; queue-only mode retains the batch silently.

Configure a Pod-scoped unattended curation schedule in the same runner. It
lists pending Candidates and evaluates them under each Pod's configured policy;
task-backed Candidates that pass Assisted Curation become accepted Pod content,
while anything requiring judgment remains in the manual review queue. Keep this
grant limited to `pod_curation` and the Pods served by that worker.

### Privacy and recovery

Interest Seeds, Source Affinities, Discovery Plans, schedules, result batches,
and reactions are private Home Node state. They never enter Pod Events,
packages, announcements, Explore artifacts, or subscription synchronization.
Back up the full node directory while stopped; restore and restart with the
same `--data-dir` to resume grants, plans, batches, and schedules together.

Example management grant:

```bash
target/release/stumble --data-dir ~/.stumble/nodes/home node harness register \
  --label "Personal discovery manager" --kind interactive \
  --capability personal_discovery_management \
  --capability feedback \
  --capability candidate_submission
```

Example worker grant:

```bash
target/release/stumble --data-dir ~/.stumble/nodes/home node harness register \
  --label "Personal discovery worker" --kind unattended \
  --capability personal_discovery_execution
```

## Personal Discovery browser sessions and source availability

The Agent Harness owns login and browser control through its User-approved
Browser Connector and Browser Grants. Stumble never receives credentials,
cookies, tokens, or raw browser state. Workers report only availability facts
(`available`, `authentication_required`, `session_expired`, `inaccessible`,
`browser_grant_ineligible`) and optional Browser Grant eligibility for planned
source neighborhoods.

Browser Grant eligibility restricts planning and execution. Taste Profile
evidence, Pod Packages, Discovery Leads, remote Index metadata, and
worker-supplied content cannot broaden Browser Grants or authorize account
mutations.

- **On-demand runs** may request User-assisted login for a valuable unavailable
  authenticated source while continuing accessible planned work. At most one
  authentication-needed notice is emitted per unavailable source state; the
  notice becomes eligible again only after availability changes (for example a
  restored session later expires).
- **Scheduled runs never wait for authentication** and never attempt login.
  They skip unavailable authenticated sources, reallocate remaining quota
  within plan policy, and complete with inspectable reasons such as
  `authentication_skipped_scheduled`. Failure of one source never discards
  valid task-bound results already collected from other sources.

Report availability with `report_discovery_source_availability` (MCP) while
holding the task lease. Inspect private notices with `list_authentication_needed_notices`.

## Scheduling fallback (Pod and Personal Discovery)

If the Agent Harness has no scheduler, the unified runner materializes due
tasks for both Pod Source Rules and Personal Discovery schedules, writes a
private `discovery_ready` event, and invokes the configured generic agent
command. It never controls a browser. See the Personal Discovery skill section
above for runner configuration and launchd installation.

## Direct two-node federation

Public exposure and later public placement withdrawal are sensitive changes:
an authorized harness creates a Pending Proposal and a separate interactive
approval harness accepts it. The Origin Node then serves the current
`stumble/1.0` identity, manifest, signed Package, and append-only Pod Events.

For an existing private Pod, request publication and independently approve its
Pending Proposal through the canonical Pod and Node workflows:

```bash
STUMBLE_HARNESS_CREDENTIAL="$PROPOSER_CREDENTIAL" \
  target/release/stumble --data-dir ~/.stumble/nodes/origin \
  pod visibility set <pod-id> --visibility public
STUMBLE_HARNESS_CREDENTIAL="$APPROVER_CREDENTIAL" \
  target/release/stumble --data-dir ~/.stumble/nodes/origin \
  node proposal approve <proposal-id>
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

The equivalent MCP flow calls `subscribe_public_pod` with that canonical URL
through `stumble-subscriber-manager`, then calls `synchronize_subscription`
there with the returned Subscription identity for incremental refreshes.
Continue to use `stumble-origin-curator` for later Candidate routing and
acceptance. Addressing the profiles this way keeps node selection in operator
configuration rather than in caller-supplied tool arguments.

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
