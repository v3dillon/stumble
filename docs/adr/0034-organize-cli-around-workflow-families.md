# Organize the Stumble CLI around workflow families

The pre-release `podctl` surface mixed User workflows, Agent Harness operations, transport launchers, development utilities, and retired compatibility commands. Replace it atomically with a local-only `stumble` CLI organized around five stable families—`node`, `pod`, `discover`, `feed`, and `sync`—and remove the old binary and command names without aliases. The public CLI is a curated workflow surface rather than a one-to-one projection of HTTP or MCP capabilities.

## Command tree

```text
stumble
├── node
│   ├── init
│   ├── show
│   ├── harness  list | show | register | revoke
│   └── proposal list | show | approve | reject
├── pod
│   ├── list | show | create | explore | subscribe | unsubscribe
│   ├── subscription set
│   ├── visibility   set
│   ├── role         list | grant | revoke
│   ├── content      list | show | add | remove
│   ├── policy       show | set
│   └── package      show | export | validate | revise
├── discover
│   ├── task      list | show | claim | renew | complete | fail
│   └── candidate list | submit | show | evaluate | route | review
├── feed
│   ├── batch    get | complete
│   ├── feedback record
│   └── taste    show | set | reset | retract
└── sync
    ├── peer list | add | remove
    └── pod  run | status
```

Command paths use resource-first grammar and short, unhyphenated words. Conventional multiword option flags may use kebab case.

## Local node and authorization

- `stumble` defaults to `~/.stumble/nodes/home`; `STUMBLE_DATA_DIR` and `--data-dir` override it.
- Only `node init` creates Home Node state, and it fails when that path is already initialized. Other commands return `node_not_initialized` for an empty path.
- Initialization stores the Home Node Owner Credential in the operating-system keychain. Local User commands retrieve it automatically. Unrestricted shell access as the same operating-system User is owner-level authority.
- A scoped adapter supplies its Harness credential. `node harness register` and `revoke` are direct, Owner-only bootstrap actions; registration returns the Harness credential once, while later reads expose only its fingerprint and metadata.
- Every other sensitive change uses a Pending Proposal. The Home Node Owner or a separately authorized interactive Agent Harness may approve it; callers never construct generic proposal documents.
- Remote operation belongs to `stumble-api` and `stumble-mcp`. The former owns HTTP serving; the latter owns streamable HTTP and stdio MCP. `stumble` has no `--api`, `serve`, or `mcp` mode.

## Machine contract

- Success writes one `{ "version": 2, "data": ... }` JSON document to stdout. Failure writes one `{ "version": 2, "error": ... }` document to stderr. Version 2 introduces the typed Candidate Submission target contract. `--format text` is an optional human rendering.
- Errors contain a stable domain code, message, and optional details. Exit codes distinguish only usage, authorization, validation or conflict, and internal failure.
- Breaking JSON changes require a new envelope version; additive fields do not.
- Every `list` accepts `--limit`, an opaque `--cursor`, and relevant filters, returning bounded `{ "items": [...], "next_cursor": ... }` data.
- Every `show` or `get` result includes `allowed_actions` for the active credential and current resource state. List items omit them.
- Scalar requests use positional identifiers and flags. Structured requests use `--input FILE|-`, with `-` reading JSON from stdin. Pod Packages remain directory-based portable artifacts.
- Non-idempotent creation and submission accept idempotency keys. Candidate Submission requires one; other operations generate and return one when omitted. Identical retries return the original result, while changed input under the same key is a conflict.
- Every Pod-targeting command accepts an exact local slug or immutable Pod ID. Subscription may also accept a canonical public Pod URL. Results include both `pod_id` and `slug`.

## Workflow semantics

- `pod create` requires explicit `private`, `invite-only`, or `public` visibility and accepts either an initial package directory or mutually exclusive `--from-pod SOURCE`. Forking is therefore a provenance-preserving creation mode. Public creation returns a Pending Proposal.
- `pod visibility set` is the later visibility transition. Expanding exposure requires approval; an authorized restriction may apply directly.
- Subscription controls Feed eligibility and grants no Pod authority. `subscribe` and `unsubscribe` replace `join`; Priority Subscription is changed through `pod subscription set`.
- Pod authority uses only Owner and Curator roles. Ordinary subscribers have no Pod Role; Member and Admin roles are removed. Role changes require approval.
- `pod explore` returns Trust Policy-filtered public Pods and sample Content Items without subscribing.
- `pod content list` exposes the complete accepted Pod stream independently of Feed selection. `add` performs Add to Pod immediately under existing curation authority. Private removal applies directly; public removal requires approval and emits a Placement Tombstone without deleting the Content Item or unrelated placements.
- `pod policy set` manages Curation Policy. Enabling Autonomous Curation requires approval; Manual and Assisted changes use normal authority.
- Package changes are Package Revisions, not storage imports. `revise` applies when policy permits or returns a Pending Proposal.
- Source Rules are the only origin of scheduled Discovery Tasks. Due work materializes automatically, so manual task creation and materialization are not public commands. One filtered task list replaces separate all/ready lists.
- Candidate operations cover submission, inspection, policy evaluation, evidence-backed routing, and manual Pod Placement review.
- `feed batch get` is the canonical Feed Batch retrieval operation. Drip remains conversational Agent Harness language, not a CLI command or alias. Item-driven blocks are Feedback Signals; deliberate bulk preference changes use `feed taste set`.
- Synchronization remains automatic. `sync pod run` is explicit recovery, peer trust changes require approval, and signed-event file manipulation is internal protocol tooling.

## Removed surface

Remove standalone tenant and raw API-token commands, generic proposal creation, transport launchers, hosted-development utilities, manual Discovery Task creation/materialization, duplicate ready-task listing, signed-event file commands, and all obsolete submission, crawling, discovery, Stumble, Brief, and block compatibility operations. Repository callers migrate in the same change; removed commands become ordinary usage errors rather than deprecated or hidden aliases.
