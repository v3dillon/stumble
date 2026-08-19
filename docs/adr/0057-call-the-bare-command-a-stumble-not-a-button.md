# Call the bare command a Stumble, not a button

ADR-0051 made the bare `stumble` command return exactly one new Content Reference per run. That behavior stands. This decision retires the "press" and "button" language that ADR-0051 used to describe it.

A Stumble is an Agent Harness action. The harness runs the bare `stumble` command and receives one outcome: the next unseen Feed Batch item, a clearly labeled Origin-signed sample from an unsubscribed public Pod, or a Caught Up report. There is no button and no press. The surface cursor, network fallback, and text-card default stay as ADR-0051 specified.

CONTEXT.md names this action Stumble. Avoid Press, button, and "hit the button" in harness instructions, CLI help, and user-facing copy.
