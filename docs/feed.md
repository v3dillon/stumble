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

MCP `get_feed_batch` arguments expose the Feed Mix and Batch Intent fields
directly; the CLI accepts the same `FeedBatchRequest` only as JSON via
`stumble feed batch get --input <file>` (omitting `--input` requests a default
batch of 7). Priority Subscription updates are a Core operation surfaced only
through CLI `stumble pod subscription set`; there is no MCP tool for them.

Each item carries its Content Reference, all contributing Accepted Placement
evidence, discovery provenance, Attention Value evidence, exploration label, current
feedback state, and permission-derived next actions. Feedback supports Save, More
like this, Less like this, Dismiss, source block, and topic block; the existing Add to
Pod operation creates an Accepted Placement while preserving provenance. Stumble
does not collect dwell time or session duration. On the bare `stumble` press,
each shown item additionally carries its locally stored assets — the generated
cover and readable snapshot, when present — resolved at presentation time; this
is press-only, and `stumble feed batch get` returns items without assets.

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

- MCP: `get_feed_batch`, `complete_feed_batch`, `record_feed_feedback`, and
  `retract_interest_seed`.
- CLI: `stumble feed batch get`, `stumble feed batch complete`,
  `stumble feed feedback record`, and `stumble feed taste retract`.

The bare `stumble` press is a CLI presentation surface composed from these
same operations: each press shows the next not-yet-shown item of the current
stable batch, completes the batch once fully shown, and requests the next one.
The press cursor is surface state kept beside the store
(`stumble_surface.json`), never domain state. When the Feed is caught up, the
press falls back to Explore — a clearly labeled Origin-signed sample from an
unsubscribed public Pod — which keeps Feed Batches subscription-only while
still giving the button a network answer.

The HTTP API serves only the node-to-node network surface (federation,
Bootstrap, and Discovery Peer routes); the Feed is a local Harness surface.

Taste Profile inspection is available through MCP (`get_taste_profile`) and CLI
(`stumble feed taste show`); explicit updates and selective/all learned reset
are CLI-only (`stumble feed taste set` and
`stumble feed taste reset`). Whole-profile operations require an unscoped
Feedback grant; Pod-scoped harnesses continue to use item-scoped feedback without
receiving access to the User's complete private profile.
