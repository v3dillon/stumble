# Complete the constrained Feed Mix

Status: complete

Blocked by: 09, 11, 12

Compose finite Feed Batches from highest-value subscribed content, controlled
Exploration Items, bounded Priority Subscription representation, and deliberate Old
Gems while preserving diversity, canonical identity, and temporary Batch Intent.

## Acceptance criteria

- [x] Default batches target 80% highest-value subscribed content, up to 10% Exploration Items, and up to 10% Old Gems.
- [x] Per-Pod and per-source caps prevent domination while unavailable categories backfill cleanly.
- [x] Eligible Priority Subscriptions receive bounded representation.
- [x] Canonical Content Items are delivered once with all contributing Accepted Placement evidence.
- [x] Exploration Items are clearly labeled and never create a Subscription implicitly.
- [x] Batch Intent remains temporary and visible in the stable batch explanation.
- [x] Old Gems return after recurrence decay or strong new evidence.
- [x] Explicit rejection and blocks prevent automatic resurfacing.
- [x] Composition tests assert observable categories and constraints rather than one score formula.

## Comments

- The primary acceptance seam is `AgentTools::get_feed_batch`; tests observe stable Feed Batch output rather than private selection helpers.
- Feed Mix, Batch Intent, and item-kind fields use serde defaults so existing local snapshots and adapter requests remain compatible.
- Completed with cap-aware category selection and backfill, private Priority Subscription configuration, recurrence-aware Old Gems, and additive structured explanations.
