Status: ready-for-agent
Blocked by: 02, 05

# Review results and close the learning loop

## Parent

Personal Discovery from User evidence PRD.

## What to build

Let the User review each Discovery Result Batch item and deliberately save, place, reinforce, reject, or ignore it, preserving the distinction between private Candidate review, Pod curation, and Taste Profile learning.

## Acceptance criteria

- [ ] Save creates an Accepted Placement in the User's private Inbox while retaining the result's original provenance.
- [ ] Add to Pod uses existing Pod Role, Harness Grant, curation, and proposal boundaries and never bypasses public-Pod policy.
- [ ] More like this and Not for me create explicit private supporting or opposing evidence for the result's eligible topics and Source Affinities.
- [ ] Ignoring an item, delivering a batch, notifying the User, or dismissing the whole batch creates no learning evidence.
- [ ] Repeating the same item action is idempotent and cannot inflate evidence; changing an action has explicit, inspectable replacement semantics.
- [ ] Item review state and batch review completion remain distinguishable from saving or Pod placement.
- [ ] User feedback changes the next eligible Discovery Plan in an explainable way while explicit blocks continue to override learning.
- [ ] A result rejected by the User cannot be immediately rediscovered through canonical or equivalent URL spelling.
- [ ] Review actions, placements, learning, and batch state commit atomically and survive restart.
- [ ] Supported adapters expose equivalent allowed actions, outcomes, authorization errors, and updated aggregate Taste Profile evidence.

## Comments

Keep private discovery review separate from Feed Batch completion and from Pod Placement review even when they reuse existing domain operations.
