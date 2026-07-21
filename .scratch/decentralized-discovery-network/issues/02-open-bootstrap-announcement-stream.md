# Operate open Bootstrap admission and Announcement Streams

Status: ready-for-agent
Blocked by: 01
Source: ../PRD.md

## What to build

Allow a Bootstrap-capable Stumble node to admit verifiable public Pod Announcements without prior Origin approval and serve their lifecycle through a neutral cursor-paginated Announcement Stream. Admission must constrain abuse without assigning trust, quality, or rank.

## Acceptance criteria

- [ ] A public Origin can submit an announcement without a User account or pre-existing Trusted Peer relationship.
- [ ] Admission verifies Origin identity, signature, canonical Pod URL, current public manifest, reachability, protocol compatibility, lease, and bounded payload size.
- [ ] Rejected submissions return a stable machine-readable reason covering malformed data, invalid identity or signature, unreachable Origin, incompatible protocol, stale lease, and rate limiting.
- [ ] Per-network and per-Origin admission limits prevent one submitter from monopolizing accepted state or a stream page.
- [ ] Canonically duplicate submissions are idempotent and materially duplicate public Pods are bounded without inventing a global quality score.
- [ ] The Announcement Stream is topic-neutral, cursor-paginated, bounded, and emits admitted additions, renewals, withdrawals, and expiry transitions.
- [ ] Cursors resume without gaps or duplicate effects across process restart and reject unknown or invalid positions safely.
- [ ] Admission, stream state, rejection audit data, and leases persist transactionally in SQLite.
- [ ] The public protocol contains no Taste Profile, Subscription, feedback, popularity, endorsement-derived authority, or personalized ranking data.

## Comments

