---
name: watch-x
description: Work one X (x.com / twitter.com) timeline or account watch from a Personal Discovery plan. Use when a Discovery Plan source neighborhood carries a watch with skill "watch-x". Opens the watch URL in the harness browser, judges a bounded scroll window against the plan, and submits only clear passes.
---

# watch-x

You are working one watch from a minimized Personal Discovery plan. The plan
neighborhood gives you the watch URL, kind, and this skill. Follow these steps
in order:

1. **Open the watch URL in the harness browser.** Use the user's existing
   logged-in session. Never ask for credentials and never store them in
   Stumble.
2. **If the signed-out wall is up, stop this watch.** Report the source with
   `stumble discover personal ...` availability as `authentication_required`
   (`report_discovery_source_availability` on the claimed task, or the
   `source_availability` field of `complete-batch`). Do not skip in silence.
   An unsigned X session is a hard stop for **this watch only** — continue
   with the other planned sources.
3. **Scroll a bounded window.** A few screens, not an endless feed. Judge each
   post against the plan's topics, neighborhoods, and blocks — and against
   `context_md` from `stumble context show` when you run interactively.
   Unattended workers judge against the plan only.
4. **Submit only clear passes** into the Discovery Result Batch with
   `stumble discover candidate submit` (kind `personal_discovery`, the claimed
   task id, allocation role from the watch neighborhood). When unsure, leave
   it out.
5. **Complete with availability facts.** Include a `source_availability`
   report for the watch source (`available`, `authentication_required`,
   `session_expired`, or `inaccessible`) so the node can persist the watch's
   last availability and raise a gap in the next brief.

Facts only: never write cookies, tokens, passwords, or raw browser state into
any Stumble field.
