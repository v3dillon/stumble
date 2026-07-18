# Deliver Pod Package revisions

Status: complete

Blocked by: 07

Replace storage-oriented package imports with validated, version-aware Pod Package read, export, and revision workflows.

## Acceptance criteria

- [x] `pod package show` reads the current or requested immutable package version.
- [x] `pod package export` writes the complete portable package artifact with provenance history.
- [x] `pod package validate` validates a directory without changing authoritative state.
- [x] `pod package revise` creates a validated Package Revision from a portable directory.
- [x] Revision detects stale base versions rather than silently overwriting newer state.
- [x] Private revisions apply under current policy; public revisions return Pending Proposals.
- [x] Package history, signatures, Source Rules, filters, context, skill, and examples survive export and revision.

## Comments

- 2026-07-18: Implementation started after issue 07 completed and the issue 08 checkpoint passed full workspace validation.
- 2026-07-18: Added executable-level package show, export, validation, and revision workflows. Revisions verify package-only signed provenance against their explicit immutable base without exporting unrelated private Pod Events; private revisions apply atomically, while public revisions remain unchanged until independent approval. Focused CLI, core package, and Pending Proposal suites pass.
