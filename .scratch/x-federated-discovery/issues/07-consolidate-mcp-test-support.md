Status: ready-for-agent

# Consolidate MCP integration test support

Blocked by: 05, 06

Extract reusable typed MCP/node test support so Origin, subscriber, and full two-node tests share setup and response handling, and the end-to-end acceptance test reads as a short scenario instead of a long transport script.

- [ ] Shared support owns temporary persistent nodes, scoped Agent Harness registration, MCP calls/results, and ephemeral Origin HTTP lifecycle.
- [ ] Raw MCP envelope traversal is localized behind typed helpers.
- [ ] Duplicated request construction and response decoding are removed from the three integration suites.
- [ ] The two-node acceptance test is decomposed into named scenario operations and remains behaviorally identical.
- [ ] Test-only abstractions remain direct and do not leak into production modules.

## Comments

- Added after the thermo-nuclear review of the first implementation.
