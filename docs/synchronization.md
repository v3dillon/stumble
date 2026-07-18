# Subscriptions and synchronization

A private Home Node subscribes to a public Pod by its canonical HTTPS address:
`https://origin.example/federation/pods/<slug>`. The Home Node only makes outbound
requests. It fetches the Origin Node identity, public manifest, and signed Pod
Events, then pins the Origin public key in a local `Subscription`.
Before inspecting or projecting events, it requires the Origin to advertise the
current `stumble/1.0` protocol. Incompatible versions fail negotiation so new
event shapes cannot be interpreted under the pre-release contract.
Loopback HTTP is accepted for a two-node local deployment; non-loopback public
Pod addresses require HTTPS. This address policy is checked before any outbound
request is attempted.

`stumble-sync::subscribe_pod_from_url` performs the initial fetch. Subsequent
calls to `stumble-sync::synchronize_subscription_from_origin` request the same
public artifacts and apply only events after the Subscription's stored content
hash cursor. A missing cursor, changed Origin identity or key, broken event
chain, invalid signature, invalid Pod Package, or manifest mismatch rejects the
complete update before projection. Retrying an already applied signed segment
is an idempotent no-op. Package versions must be positive, monotonic, and
immutable; the complete segment is preflighted and projected atomically.

The Node Agent uses that high-level Origin workflow for normal automatic
Subscription refreshes. Operators may recover one stalled Subscription with
`stumble sync pod run <pod> --peer <peer-id>`. The selected peer must match the
Subscription's pinned canonical Origin Node ID and public key before any event
fetch is applied. `stumble sync pod status <pod>` exposes the stored cursor,
verification result, latest event, last success, and the latest persisted
failure with a retry action. Peer additions and removals are Trust Policy
changes and therefore remain Pending Proposals until independently approved.
The CLI deliberately has no signed-event file export, import, or verification
commands.

Only signed Pod metadata, signed Pod Package versions, and Accepted Placements
with their reference-first Content Items are projected. Candidate state and
unaccepted legacy submissions are not Feed-eligible. The synchronized SQLite
projection remains usable when the Origin Node is offline. Subscriptions,
Taste Profiles, Feedback Signals, Feed history, Harness Grants, and other
private Home Node state are never exposed from a remote Pod's federation
surface, and synchronized Pods are not re-published by the Home Node.
Origin Pod identities are mapped to distinct local IDs, and a remote slug or ID
collision cannot alias or overwrite a locally authoritative Pod.

Public placement withdrawal follows the same verified event chain. An Origin
Node first creates a Pending Proposal; independent approval appends a signed
`placement_tombstoned` Pod Event containing the withdrawn Accepted Placement
and its reference-first audit snapshot. A subscriber removes only that Origin
Pod's Feed eligibility. An independent local Save remains available through
`saved_content_references`, and an Add to Pod placement remains accepted with
its original `origin_placements` plus the later `origin_withdrawals` evidence.
The signed acceptance and tombstone events remain append-only. A Content
Reference is retained whenever an active placement, private Save, or required
withdrawal audit still refers to it.
