Status: ready-for-agent

# Canonicalize media references and resolve later evidence

Blocked by: 04

Make media-reference validity and canonical identity explicit in the domain, and define deterministic behavior when later canonical-deduplicated submissions add media after a Content Item is already accepted.

- [ ] Invalid media URLs cannot inhabit accepted or persisted domain state.
- [ ] Equivalent HTTP(S) URL spellings deduplicate through one canonical URL policy.
- [ ] Media evidence resolution is explicit and deterministic across all submissions.
- [ ] Later evidence enriches an existing accepted Content Reference atomically and emits the required signed metadata update, or an equally explicit documented freeze policy is enforced.
- [ ] Tests prove pre-acceptance union, post-acceptance enrichment, synchronization, restart, and conflict behavior.

## Comments

- Added after the thermo-nuclear review of the first implementation.
