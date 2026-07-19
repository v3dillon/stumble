Status: ready-for-agent

# Subscribe and synchronize a public Pod through MCP

Blocked by: 01

Deliver a subscriber-node MCP workflow that accepts a canonical public Pod URL, subscribes outbound through the existing signed federation contract, refreshes the Subscription incrementally, and reports verified synchronization results.

- [ ] Direct-URL subscription is exposed as an asynchronous MCP operation.
- [ ] Incremental Origin synchronization is exposed by Subscription identity.
- [ ] Both operations require Subscription Management.
- [ ] Invalid URLs, identity changes, signature failures, and event-chain failures preserve canonical errors.
- [ ] Tests use separate Origin and Home nodes with a real ephemeral HTTP listener.

## Comments

