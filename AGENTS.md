## Using Stumble from a harness

If you are operating Stumble for a User (not developing it), read the root `SKILL.md`. The short version: the harness owns the browser, Stumble owns the collection — `stumble node init` once, `stumble add <url>` to save what the User shares, `stumble feed batch get` to read their Feed back.

## Agent skills

### Issue tracker

Issues are tracked as local Markdown under `.scratch/`; external PRs are not a request surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default triage roles without renaming. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository with a root `CONTEXT.md` and system-wide ADRs under `docs/adr/`. See `docs/agents/domain.md`.
