# Subscriptions and synchronization

A private Home Node subscribes to a public Pod by its canonical HTTPS address:
`https://origin.example/federation/pods/<slug>`. The Home Node only makes outbound
requests. It fetches the Origin Node identity, public manifest, and signed Pod
Events, then pins the Origin public key in a local `Subscription`.
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

Only signed Pod metadata, signed Pod Package versions, and Accepted Placements
with their reference-first Content Items are projected. Candidate state and
unaccepted legacy submissions are not Feed-eligible. The synchronized SQLite
projection remains usable when the Origin Node is offline. Subscriptions,
Taste Profiles, Feedback Signals, Feed history, Harness Grants, and other
private Home Node state are never exposed from a remote Pod's federation
surface, and synchronized Pods are not re-published by the Home Node.
Origin Pod identities are mapped to distinct local IDs, and a remote slug or ID
collision cannot alias or overwrite a locally authoritative Pod.
