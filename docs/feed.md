# Finite local Feed

`get_feed_batch` creates a finite, stable Feed Batch from locally stored Accepted
Placements. Inclusion marks every item Delivered immediately. Calling the operation
again returns the same current batch until the harness completes it; newly accepted
content waits for the next batch. When no eligible item remains, the batch has the
explicit `caught_up` state and no items.

The default recurrence penalty is 30 days and can be changed per request. Delivery
history downranks recent repetition rather than permanently excluding an item.
Dismissed and Less-like-this items, blocked sources, and blocked topics are excluded
from automatic delivery.

Each item carries its Content Reference, all contributing Accepted Placement
evidence, discovery provenance, Attention Value evidence, exploration label, current
feedback state, and permission-derived next actions. Feedback supports Save, More
like this, Less like this, Dismiss, source block, and topic block; the existing Add to
Pod operation creates an Accepted Placement while preserving provenance. Stumble
does not collect dwell time or session duration.

Harness adapters expose the same contract:

- HTTP: `GET /feed`, `POST /feed/:id/complete`, and
  `POST /feed/items/:id/feedback`.
- MCP: `get_feed_batch`, `complete_feed_batch`, and `record_feed_feedback`.
- CLI: `podctl feed`, `podctl complete-feed`, and `podctl feed-feedback`.
