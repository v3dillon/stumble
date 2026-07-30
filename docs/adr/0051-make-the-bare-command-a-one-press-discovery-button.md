# Make the bare command a one-press discovery button

Bare `stumble` is the button: every press presents exactly one new Content Reference — link, summary, and local assets — never repeating until the pool is dry. A press walks the current stable Feed Batch one not-yet-shown item at a time, completes the batch once fully shown, and rolls into the next composition. When the Feed is caught up, the press falls back to the Explore surface and presents one clearly labeled Origin-signed sample from an unsubscribed public Pod; Feed Batches themselves stay subscription-only. With nothing new anywhere, the press reports caught up with next steps rather than repeating content.

The press cursor is presentation state stored beside the Home Node store (`stumble_surface.json`), never domain state: delivery, completion, and feedback facts remain exclusively in the store through the existing Feed operations. The fallback issues no interest-derived remote queries — it ranks already-synchronized announcements locally and fetches samples by announcement identity only.

As the one human-facing surface, the bare press renders a text card by default while still honoring `--format json`; every subcommand keeps the JSON-first machine contract of ADR-0034.
