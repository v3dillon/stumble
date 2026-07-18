# Finite local Feed

`get_feed_batch` creates a finite, stable Feed Batch from locally stored Accepted
Placements. Inclusion marks every item Delivered immediately. Calling the operation
again returns the same current batch until the harness completes it; newly accepted
content waits for the next batch. When no eligible item remains, the batch has the
explicit `caught_up` state and no items.

The default recurrence penalty is 30 days, can be edited in the User's private
Taste Profile, and can be overridden per request. Delivery history downranks recent
repetition rather than permanently excluding an item.
Dismissed and Less-like-this items, blocked sources, and blocked topics are excluded
from automatic delivery.

## Constrained Feed Mix

Each batch records the exact `FeedMix` used to compose it. Defaults target 80%
highest-value subscribed content, up to 10% clearly labeled Exploration Items, and
up to 10% Old Gems. Per-Pod and per-source caps apply across the blend; unused
category capacity backfills from the next highest Attention Value candidates without
relaxing those diversity caps.

Priority Subscriptions are configured with `set_priority_subscription`. One eligible
item per Priority Subscription is selected ahead of ordinary subscribed content,
bounded within the subscribed-content target so priority does not become the whole Feed.
Canonical Content Items remain deduplicated and retain all contributing Accepted
Placement evidence.

`BatchIntent` supplies temporary focus and avoidance topics. It is recorded on the
stable batch and in ranking explanations, but is never written into the User's Taste
Profile. Previously Delivered Items become Old Gems after the recurrence window
decays, after a new independent Pod Placement appears, or after the User both saves
the item and requests More like this.

HTTP Feed queries, MCP `FeedBatchRequest` arguments, and CLI `feed` flags expose the
same Feed Mix and Batch Intent fields. Priority Subscription updates are available at
`POST /subscriptions/:pod_id/priority`, MCP `set_priority_subscription`, and CLI
`priority-subscription`.

Each item carries its Content Reference, all contributing Accepted Placement
evidence, discovery provenance, Attention Value evidence, exploration label, current
feedback state, and permission-derived next actions. Feedback supports Save, More
like this, Less like this, Dismiss, source block, and topic block; the existing Add to
Pod operation creates an Accepted Placement while preserving provenance. Stumble
does not collect dwell time or session duration.

## Private Taste Profile

The Taste Profile combines explicit interests, topic and source blocks, and the
default recurrence window with locally learned topic and source weights. Explicit
blocks always exclude matching content, even when learned evidence is positive.
Feedback Signals and Add to Pod contribute aggregate evidence; one weak action is
inspectable but has zero ranking weight until another action corroborates it.

Learned evidence exposes action categories and counts, not Content Item identifiers,
URLs, reasons, or raw feedback history. A User may reset one topic or source weight,
or the complete learned layer. Profile settings, weights, and evidence live only in
the Home Node store and are never included in Pod Packages, Pod Events, manifests,
or other federation surfaces.

Harness adapters expose the same contract:

- HTTP: `GET /feed`, `POST /feed/:id/complete`, and
  `POST /feed/items/:id/feedback`.
- MCP: `get_feed_batch`, `complete_feed_batch`, and `record_feed_feedback`.
- CLI: `stumble feed batch get`, `stumble feed batch complete`, and
  `stumble feed feedback record`.

Taste Profile inspection, explicit updates, and selective/all learned reset are
available through HTTP (`GET/PATCH /taste-profile` and
`POST /taste-profile/learned/reset`), MCP (`get_taste_profile`,
`update_taste_profile`, and `reset_learned_taste`), and CLI
(`stumble feed taste show`, `stumble feed taste set`, and
`stumble feed taste reset`). Whole-profile operations require an unscoped
Feedback grant; Pod-scoped harnesses continue to use item-scoped feedback without
receiving access to the User's complete private profile.
