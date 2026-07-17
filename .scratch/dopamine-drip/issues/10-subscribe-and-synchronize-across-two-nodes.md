# Subscribe and synchronize across two nodes

Status: complete

Blocked by: 03, 07, 08

## Acceptance

- [x] An Origin Node can publish a public Pod through an approved visibility change.
- [x] A Home Node can subscribe using the public Pod URL without becoming publicly reachable.
- [x] Signed Pod Package versions and append-only Pod Events are verified before projection.
- [x] Synchronization resumes incrementally from a stored cursor and is idempotent.
- [x] Only Accepted Placements and permitted Content References synchronize.
- [x] Remote content becomes Feed-eligible while local Taste Profile and Feedback Signals remain private.
- [x] An unavailable Origin Node does not make already synchronized Feed content unusable.
- [x] The two-node behavior is covered at the primary temporary-SQLite acceptance seam.

## Comments

- Implementation uses two real `AgentTools` instances backed by separate temporary SQLite Home Node stores, with the Home Node fetching from an actual loopback Origin HTTP listener.
- Direct-address synchronization pins the Origin identity and key, verifies the complete signed chain and Package version before projection, and persists a content-hash cursor for incremental refresh.
- Regression coverage proves exact replay and empty incremental refresh are no-ops, malformed or tampered events fail before projection, unaccepted submissions stay private, and synchronized Feed content remains usable after the Origin listener stops.
- URL policy is enforced before outbound I/O; package updates are preflighted for positive monotonic immutable versions; projection is atomic; and remote Pod slug/UUID collisions cannot alias local authority.
- Remote Content Item IDs map through the tenant-and-Origin-scoped federation map; they cannot alias or overwrite local or cross-tenant records, while same-tenant canonical URLs still deduplicate.
