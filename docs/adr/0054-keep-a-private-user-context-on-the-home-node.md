# 54. Keep a private User Context on the Home Node

Date: 2026-08-15

## Status

Accepted

## Context

Pods carry a `CONTEXT.md` that tells an agent what the Pod is about. The
person had no equivalent: taste lives in the Taste Profile as structured
evidence, but there was no place for the user's own prose — who they are,
what they care about, what to never bring them. Agents reconstructed this
from scattered signals on every session, and some of it leaked into ad hoc
harness memory outside the node's control.

## Decision

The Home Node stores one private User Context per User: markdown prose
(`context_md`), read and written only through the interactive, unscoped
Personal Discovery management policy. `stumble context show` returns it in
one briefing packet together with the Taste Profile, the User's watches, and
Personal Discovery readiness. Only the interactive User (or a draft the User
accepted) writes the prose; agent finds never train it.

User-scoped watches follow the same rule: they live on the User, not on a Pod,
and are not Pod Source Rules. Due watches enter the minimized Discovery Plan
as first-class neighborhoods carrying only the URL, kind, and skill.

## Consequences

- The User Context and watches are private, inspectable local state. They
  never federate and never appear in announcements, Explore, or the Index.
- Unattended `personal_discovery_execution` workers cannot read the User
  Context, the Taste Profile, or the watch list. They see only the minimized
  Discovery Plan, which now includes due watch neighborhoods.
- The harness reads one packet before it saves, discovers, or writes a brief,
  instead of inventing a personality from partial signals.
