# Rotate Discovery Peers and survive Bootstrap outages

Status: ready-for-agent
Blocked by: 03, 06
Source: ../PRD.md

## What to build

Allow Home Nodes to maintain a small automatic outbound Discovery Peer set, synchronize neutral announcements through it, and keep established discovery functioning when configured Bootstrap Nodes are unavailable.

## Acceptance criteria

- [ ] A Home Node learns signed peer advertisements from Bootstrap Nodes and existing Discovery Peers and verifies identity, capability, reachability, protocol version, and lease locally.
- [ ] Peer selection is bounded, randomized with deterministic test control, and does not imply Trusted Peer status.
- [ ] The Home Node persists a small rotating outbound set with per-peer stream cursor, provenance, health, and last-success state.
- [ ] Peer synchronization accepts only unchanged valid Origin-signed announcement lifecycle artifacts and never grants access to private or administrative state.
- [ ] Invalid data, flooding, incompatible versions, expired advertisements, or repeated transport failures cause bounded backoff and automatic local eviction.
- [ ] An independently learned announcement remains eligible when one delivery source disappears, while provenance records every current source.
- [ ] Established Home Nodes continue receiving new announcements through viable peers while every configured Bootstrap is unavailable.
- [ ] A fresh node with no viable Bootstrap reports degraded discovery clearly while preserving direct Pod URL operation.
- [ ] Users can disable automatic peer gossip without deleting cached audit state or affecting Bootstrap and direct-address paths.
- [ ] Rotation, eviction, cursor resume, and bootstrap-outage behavior survive process restart without blocking the async runtime.

## Comments

