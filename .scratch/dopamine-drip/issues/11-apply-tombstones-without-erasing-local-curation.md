# Apply tombstones without erasing local curation

Status: complete

Blocked by: 10

Synchronize Origin Pod withdrawals as signed Placement Tombstones without deleting a User's independent local Save or Add to Pod placement. Preserve origin placement evidence and append-only withdrawal history, and remove Feed eligibility only for the withdrawn origin placement.

## Acceptance criteria

- An Origin Node can propose and independently approve withdrawal of a public placement.
- A subscribed Home Node verifies and incrementally applies the signed Placement Tombstone.
- The withdrawn placement no longer contributes Feed eligibility.
- Local Saves and Add to Pod placements survive with origin-withdrawal provenance.
- Content References are purged only when no active placement, Save, or required audit record retains them.
- Origin event history remains append-only.

## Comments

- Implementation uses the two-`AgentTools`, temporary-SQLite acceptance seam specified by the PRD.
- Completed with signed `placement_tombstoned` events, atomic verified projection, SQLite persistence, and provenance-preserving Save and Add to Pod behavior.
