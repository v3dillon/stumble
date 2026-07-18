# Contract the old public surface

Status: complete

Blocked by: 14

Complete the expand–migrate–contract transition by deleting the old executable and every noncanonical public parser operation.

## Acceptance criteria

- [x] The `podctl` binary target is removed.
- [x] Flat command names, hidden aliases, and compatibility shims are removed from the parser.
- [x] Server launchers, remote API flags, tenant commands, raw token commands, and generic proposal creation are absent.
- [x] Manual task creation/materialization, duplicate ready lists, and signed-event file commands are absent.
- [x] Obsolete Submission, crawler, discovery, Stumble, Brief, and standalone block operations are absent.
- [x] Removed operations return ordinary structured usage errors rather than retirement-specific compatibility behavior.
- [x] Public help output contains exactly the five accepted top-level families.

## Comments

- Implementation started after issue 14 completed at `06715d6`; issue 14's focused suites and `cargo fmt --check && cargo test --workspace` passed.
- Contracted the expand-phase bridge by removing the `podctl` target and source, making `stumble` the default executable, and removing the generated help pseudo-command so the public command catalog contains only the five accepted workflow families.
- Converted legacy CLI compatibility assertions into executable-boundary tests for ordinary versioned `usage_error` responses while retaining the HTTP and MCP retirement-contract coverage owned by those adapters.
- Focused contraction suites passed: `stumble_shell`, `legacy_contracts`, `migration_audit`, `discovery_task_workflows`, `synchronization_workflows`, and `feed_workflows`.
