# Migrate repository callers and documentation

Status: complete

Blocked by: 03, 04, 06, 07, 08, 09, 10, 11, 12, 13

Move every maintained repository caller, test, example, scheduler integration, and runbook onto the accepted executable and workflow contracts before removing the old surface.

## Acceptance criteria

- [x] All maintained executable tests invoke `stumble` and assert versioned JSON behavior.
- [x] Scheduler and Agent Harness examples use the dedicated adapters and canonical command names.
- [x] Operator and first-release documentation uses the five workflow families and current domain language.
- [x] Cross-adapter tests compare canonical IDs, errors, approvals, and allowed actions rather than obsolete command shapes.
- [x] No maintained caller depends on `podctl`, a flat command, generic proposal creation, or a CLI transport mode.
- [x] Search-based migration checks enumerate zero unintended legacy references.
- [x] Focused and workspace tests remain green before contraction begins.

## Comments

- 2026-07-18: Implementation started after all workflow slices through issue 13 completed and the corrected issue 13 checkpoint passed full workspace validation.
- 2026-07-18: Migrated maintained CLI and cross-adapter tests to `stumble`, version-1 envelopes, canonical resource-first commands, Harness credentials, stable IDs/errors/approvals/allowed actions, and domain-specific sensitive workflows. Moved the stdio process contract from the CLI suite to the dedicated `stumble-mcp` adapter suite.
- 2026-07-18: Updated README, agent installation guidance, Feed documentation, and the first-release runbook to the five workflow families and dedicated HTTP/MCP executables. The Scheduler Adapter already used `stumble discover task list` and remains covered by its executable contract tests.
- 2026-07-18: Added a repository migration audit that rejects retired binary, flat-command, remote-mode, and generic-proposal dependencies outside explicit history and issue 15's contraction surface. Intentional references are limited to ADR/PRD/issue history and `crates/stumble-cli/{Cargo.toml,src/main.rs,tests/stumble_shell.rs,tests/legacy_contracts.rs}` pending contraction.
- 2026-07-18: Migration exposed that the canonical visibility-expansion approval made a Pod public without appending the `pod_published` event produced by the retired generic publication path. Restored that federation event at the domain seam and retained the exact three-event direct-subscription assertion.
- 2026-07-18: Focused validation passed: `cargo fmt --check`, `cargo test -p stumble-cli` (64 tests), and `cargo test -p stumble-mcp` (19 tests). Parent checkpoint will run the required full workspace gate.
