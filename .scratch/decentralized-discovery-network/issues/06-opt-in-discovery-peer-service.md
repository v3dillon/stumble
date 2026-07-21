# Opt a reachable node into Discovery Peer service

Status: ready-for-agent
Blocked by: 02
Source: ../PRD.md

## What to build

Let an ordinary node explicitly contribute announcement availability without becoming an Origin proxy, content Relay, or Trusted Peer. Enabled nodes advertise a narrow signed capability and serve bounded public discovery artifacts; disabled nodes remain outbound-only.

## Acceptance criteria

- [ ] A newly initialized Home Node does not bind, advertise, or accept an inbound discovery service by default.
- [ ] An authorized User can explicitly enable and disable announcement serving through supported operator surfaces.
- [ ] Enabling requires a declared public endpoint and successful verification of node identity, protocol compatibility, HTTPS policy outside loopback, and external reachability.
- [ ] A verified node produces a signed expiring Discovery Peer Advertisement containing only identity, endpoint, protocol version, announcement-serving capability, and expiry.
- [ ] A Bootstrap Node openly admits a valid peer advertisement after reachability verification and rejects forged, stale, incompatible, private, or unreachable advertisements.
- [ ] An enabled Discovery Peer serves bounded Announcement Stream pages while preserving Origin announcement bytes and signatures unchanged.
- [ ] An enabled Discovery Peer serves small randomized bounded samples of current peer advertisements without rank or trust assertions.
- [ ] Discovery Peer endpoints expose no Pod Events, Subscriptions, Taste Profile, feedback, credentials, private projections, or administrative capability.
- [ ] Disabling service stops advertisement renewal and inbound serving without affecting outbound discovery or direct Pod synchronization.
- [ ] Opt-in state, advertisement lease, and serving cursors survive SQLite restart.

## Comments

