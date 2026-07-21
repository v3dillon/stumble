# Search through replaceable Index Nodes privately

Status: ready-for-agent
Blocked by: 02, 03
Source: ../PRD.md

## What to build

Provide efficient intentional public-Pod search through replaceable Index Nodes while preserving the rule that passive personalization is local. Only words explicitly authored by the User may become a remote search query, and remote scores must not control local eligibility or order.

## Acceptance criteria

- [ ] An Index-capable node searches its admitted valid announcement catalog for an explicit bounded query.
- [ ] A Home Node sends a remote query only from an explicit User-authored Explore action, never from Taste Profile, Source Affinity, Subscription, feedback, or Discovery Plan inference.
- [ ] Search results contain current Origin-signed announcements and retrieval evidence without quality, trust, popularity, or personalized authority fields.
- [ ] The Home Node verifies returned announcements, applies Trust Policy, discards remote ordering authority, and recomputes relevance locally.
- [ ] Multiple configured Index Nodes are supported and removing one excludes results known only through it without affecting independent copies.
- [ ] Empty, oversized, malformed, rate-limited, and incompatible searches return bounded typed outcomes.
- [ ] Explicit search processing requires no User account or stable User identifier and does not create retained product analytics state.
- [ ] Search and local result provenance survive restart where persistence is required for audit and replacement behavior.
- [ ] HTTP, CLI, and Agent Harness-facing Explore behavior uses the same domain contract.

## Comments

