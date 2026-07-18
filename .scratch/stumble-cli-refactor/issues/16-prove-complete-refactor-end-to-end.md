# Prove the complete refactor end to end

Status: complete

Blocked by: 15

Verify the complete first-release User and Agent Harness journey through the new executable, dedicated adapters, corrected domain relationships, and versioned machine contracts.

## Acceptance criteria

- [x] Two temporary nodes initialize explicitly and authenticate through isolated Owner credential stores.
- [x] Scoped Harnesses create and curate Pods, manage packages, claim Source Rule-derived tasks, and submit Candidates through approved workflows.
- [x] Subscription, synchronization, Explore, Feed, feedback, Priority Subscription, and Taste Profile behavior remain correct.
- [x] Add to Pod and approved public removal preserve canonical identity, provenance, and Placement Tombstones.
- [x] Pending Proposals enforce independent approval for sensitive operations while Owner Harness bootstrap remains direct.
- [x] HTTP, streamable HTTP MCP, stdio MCP, and CLI results remain semantically equivalent where capabilities overlap.
- [x] Restart, migration, idempotent retry, cursor pagination, signature rejection, revocation, and privacy behavior are verified.
- [x] `cargo fmt --check` and `cargo test --workspace` pass without skipped or weakened tests.

## Comments

- Implementation started after issue 15 completed at `d59f2d4`; issue 15's focused contraction suites and `cargo fmt --check && cargo test --workspace` passed.
- The complete two-node first-release test now initializes and authenticates both nodes through the real `stumble` executable and separate isolated Owner credential stores before exercising package, discovery, curation, federation, Feed, approval, restart, signature, and privacy behavior.
- Focused CLI, core, HTTP, streamable HTTP MCP, and stdio MCP integration suites passed. Final validation: `cargo fmt --check && cargo test --workspace` passed.
