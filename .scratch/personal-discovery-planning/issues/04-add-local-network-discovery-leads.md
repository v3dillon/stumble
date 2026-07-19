Status: ready-for-agent
Blocked by: 03

# Add local Stumble-network Discovery Leads

## Parent

Personal Discovery from User evidence PRD.

## What to build

Use verified public Stumble metadata as an additional reservoir of Discovery Leads, match those leads against private interests on the Home Node, and incorporate qualified adjacent source neighborhoods into Personal Discovery Plans without exporting private queries or changing Subscriptions.

## Acceptance criteria

- [ ] Verified Pod Announcements, bounded Explore samples, endorsements, and locally available public Content References can produce generic Discovery Leads with provenance.
- [ ] Invalid, stale, blocked, untrusted, or withdrawn public metadata cannot influence a plan.
- [ ] Relevance to the Taste Profile and Source Affinities is recomputed locally; remote scores are not authoritative.
- [ ] Autonomous planning sends no profile-derived topics, Interest Seeds, Source Affinities, or matching queries to an Index Node or peer.
- [ ] An explicit User-authored Explore query remains distinct and may use the existing public discovery contract.
- [ ] Selecting a network-derived lead does not create a Subscription, import private state, accept a Placement, or grant browser authority.
- [ ] Network leads participate only in the adjacent-exploration allocation unless separately corroborated by User evidence.
- [ ] Plans explain network provenance without exposing private matching inputs to public serialization.
- [ ] Restart, trust-policy changes, removal of an Index, and replacement of equivalent verified metadata produce deterministic local results.
- [ ] Network and federation privacy tests prove that every new private type remains absent from outbound artifacts and requests.

## Comments

Preserve the replaceable, non-authoritative Index and local Trust Policy decisions in the existing discovery ADRs.
