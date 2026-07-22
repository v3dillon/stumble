# Opt a reachable node into Discovery Peer service

Status: ready-for-agent
Blocked by: 02
Source: ../PRD.md

## What to build

Let an ordinary node explicitly contribute announcement availability without becoming an Origin proxy, content Relay, or Trusted Peer. Enabled nodes advertise a narrow signed capability and serve bounded public discovery artifacts; disabled nodes remain outbound-only.

## Acceptance criteria

- [x] A newly initialized Home Node does not bind, advertise, or accept an inbound discovery service by default.
  - Evidence: `discovery_peer_service` defaults disabled; `newly_initialized_home_node_is_outbound_only_for_discovery`; well-known omits peer endpoints; `peer_announcement_stream` → `PeerServiceDisabled`
- [x] An authorized User can explicitly enable and disable announcement serving through supported operator surfaces.
  - Evidence: `AgentTools::enable_discovery_peer_service` / `disable_discovery_peer_service` (Administration); HTTP `POST|DELETE /home/discovery-peer`; test `authorized_user_can_enable_and_disable_announcement_serving`
- [x] Enabling requires a declared public endpoint and successful verification of node identity, protocol compatibility, HTTPS policy outside loopback, and external reachability.
  - Evidence: `enable_discovery_peer_service` + `normalize_discovery_peer_endpoint` + `DiscoveryPeerProbe`; tests `enable_requires_endpoint_identity_protocol_https_and_reachability`, `enable_rejects_private_and_insecure_endpoints`
- [x] A verified node produces a signed expiring Discovery Peer Advertisement containing only identity, endpoint, protocol version, announcement-serving capability, and expiry.
  - Evidence: `DiscoveryPeerAdvertisement` + `sign_discovery_peer_advertisement`; test `enable_requires_public_endpoint_and_produces_signed_ad`
- [x] A Bootstrap Node openly admits a valid peer advertisement after reachability verification and rejects forged, stale, incompatible, private, or unreachable advertisements.
  - Evidence: `admit_discovery_peer_advertisement` / `POST /bootstrap/peer-advertisements`; tests `bootstrap_admits_valid_peer_ads_and_rejects_invalid`, `bootstrap_admits_valid_peer_ad_and_rejects_forged_stale_unreachable`
- [x] An enabled Discovery Peer serves bounded Announcement Stream pages while preserving Origin announcement bytes and signatures unchanged.
  - Evidence: `read_peer_announcement_stream` / `GET /discovery/peer/announcements/stream`; tests assert byte-identical Origin payloads
- [x] An enabled Discovery Peer serves small randomized bounded samples of current peer advertisements without rank or trust assertions.
  - Evidence: `sample_discovery_peer_advertisements` / `GET /discovery/peer/advertisements`; `peer_advertisement_sample_is_public_only`
- [x] Discovery Peer endpoints expose no Pod Events, Subscriptions, Taste Profile, feedback, credentials, private projections, or administrative capability.
  - Evidence: peer serve paths return only `AnnouncementStreamPage` / `DiscoveryPeerAdvertisementSample`; privacy assertions in serve tests
- [x] Disabling service stops advertisement renewal and inbound serving without affecting outbound discovery or direct Pod synchronization.
  - Evidence: `disable_discovery_peer_service` clears lease; test `disable_stops_renewal_and_inbound_without_affecting_outbound_bootstrap`
- [x] Opt-in state, advertisement lease, and serving cursors survive SQLite restart.
  - Evidence: `opt_in_state_ad_lease_and_serving_cursors_survive_sqlite_restart` (`discovery_peer_service`, stream sequence, projected entries)

## Comments

- Implemented in `crates/stumble-core/src/discovery_peer/` (`advertise`, `admit`, `serve`, `endpoint`, `probe`, `types`) with thin `AgentTools` and HTTP wiring.
- Docs: `docs/discovery.md` (Opt-in Discovery Peer service).
