# Establish the versioned Stumble CLI shell

Status: complete

Blocked by: None

Introduce the new `stumble` executable beside the old parser temporarily and establish the shared command, input, output, pagination, and error contracts that later workflow slices will use.

## Acceptance criteria

- [x] The executable exposes only the five top-level families `node`, `pod`, `discover`, `feed`, and `sync`.
- [x] Command paths use short, resource-first, unhyphenated words.
- [x] Successful commands emit one version-1 JSON envelope on stdout; failures emit one version-1 JSON envelope on stderr.
- [x] Domain error codes and coarse exit-status categories are stable and covered by executable-level tests.
- [x] Structured input supports `--input FILE|-`, and optional text rendering is derived from the same result data.
- [x] Shared cursor pagination and allowed-action response shapes are available for subsequent commands.
- [x] The old executable may coexist only as an explicit expand-phase bridge and remains behaviorally unchanged in this ticket.

## Comments

- 2026-07-18: Implementation started from the initial ready frontier.
- 2026-07-18: Added the five-family `stumble` shell, shared version-1 envelopes, stable exit categories, file-or-stdin JSON input, text rendering, cursor pages, allowed-action details, and executable-level contract coverage. The legacy `podctl` source and behavior remain unchanged as the expand-phase bridge.
