# Let harnesses choose source access

Stumble owns what to seek and how finds enter the node. The Agent Harness owns how it reads sources. Stumble does not ship platform-specific browse skills; the shipped `watch-x` skill is removed. Stumble does not default a harness skill for x.com or twitter.com watches. An Agent Harness reads sources with whatever access it already has: official APIs or plugins, its own browser sessions, or search.

Discovery instructions name the Discovery Plan (topics, source neighborhoods, due watches) and the Candidate / `stumble add` contract. They do not name a browser, a scroll, or an X plugin as the method. Provenance `discovery_method` stays a free string with no canonical value.

Watches remain User-scoped source neighborhoods: a trusted URL, account, or site the harness reads with its own tools. The optional `skill` field stays as a passthrough, stored only when the caller sets it. Browser Grant and source-availability reporting stay as optional access control (ADR-0025); they are not a forced X client.

This decision supersedes the forced method in SKILL.md, the `skills/watch-x` package, and the x.com skill auto-default. It does not supersede ADR-0025.
