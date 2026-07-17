# Discover Pods through the Stumble Substrate

Status: complete

Blocked by: 10

Exchange compact signed Pod Announcements through explicitly trusted peers and replaceable Index Nodes while preserving direct Pod addressing. Apply each Home Node's local Trust Policy and optional Pod Endorsements when returning intentional Explore results.

## Acceptance criteria

- [x] Public Origin Nodes produce compact signed Pod Announcements without exporting full Pod content.
- [x] Trusted peers can exchange and relay announcements without becoming authoritative.
- [x] Optional Index Nodes aggregate announcements and expose replaceable search results.
- [x] Direct Pod URLs continue to work when Index Nodes are absent.
- [x] Users can configure trusted peers and Index Nodes and locally block Pods, nodes, sources, and topics.
- [x] Signatures prove origin without assigning a global quality score.
- [x] Pod Endorsements are optional local ranking evidence rather than universal reputation.
- [x] Explore can return public Pods and sample Content References beyond current Subscriptions.

## Comments

- The primary acceptance seam is independent `AgentTools` instances backed by temporary SQLite databases.
- Broad gossip, Relay Node publishing, constrained Feed Mix, and production-scale Index Node operation remain out of scope.
- Completed with origin-signed announcements and endorsements, explicit trusted-peer relay, optional announcement indexes, independently approved local Trust Policies, policy-filtered Explore samples, and restart-safe SQLite persistence.
