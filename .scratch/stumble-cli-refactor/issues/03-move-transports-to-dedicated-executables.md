# Move transports to dedicated executables

Status: complete

Blocked by: 01

Give each long-running transport one dedicated process boundary while keeping `stumble` a one-shot local workflow CLI.

## Acceptance criteria

- [x] `stumble-api` owns HTTP serving without relying on a CLI server mode.
- [x] `stumble-mcp` supports both streamable HTTP and stdio MCP.
- [x] Stdio MCP preserves authentication, scoped tool discovery, structured calls, and revocation behavior.
- [x] `stumble` exposes no `serve`, `mcp`, or remote `--api` mode.
- [x] Process-level tests cover both MCP transports and the HTTP process.
- [x] Transport diagnostics remain on stderr and never corrupt protocol stdout.

## Comments

- 2026-07-18: Implementation started after issues 01 and 02 passed full workspace validation.
- 2026-07-18: Added dedicated `stumble-mcp` HTTP and stdio selection, per-message stdio authentication, real-process transport coverage, and stderr-only lifecycle diagnostics. Focused `stumble-mcp`, `stumble-api`, and `stumble_shell` tests pass.
- 2026-07-18: Parent review corrected both HTTP executables to open only initialized Home Nodes and added process regressions proving transports never create state implicitly.
