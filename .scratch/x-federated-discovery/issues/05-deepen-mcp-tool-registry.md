Status: ready-for-agent

# Deepen the MCP tool registry and async scheduler

Blocked by: 04

Replace the parallel string catalogs, capability descriptors, dispatch matches, and async classifiers with one typed MCP tool-definition interface, while ensuring outbound network work is asynchronous and all synchronous core/store work runs on the blocking executor.

- [ ] One canonical typed definition owns each tool name, capability, schema metadata, and handler kind.
- [ ] Tool discovery, supported-name introspection, authorization filtering, and dispatch derive from that definition.
- [ ] Direct subscription and incremental synchronization separate asynchronous fetch from blocking core application.
- [ ] No SQLite/store projection or persistence runs directly on a Tokio worker.
- [ ] Existing HTTP and stdio behavior and errors remain unchanged through public tests.

## Comments

- Added after the thermo-nuclear review of the first implementation.
