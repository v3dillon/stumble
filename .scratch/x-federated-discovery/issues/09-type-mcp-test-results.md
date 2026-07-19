Status: ready-for-agent

# Type MCP integration-test results

Blocked by: 07

Replace the thin raw-JSON MCP tool-result wrapper with typed test decoders for the workflows used by the two-node scenario, keeping transport envelope traversal in one place.

- [ ] Typed decoders cover Pod creation, proposal decisions, Candidate submission, Discovery Tasks, subscription/synchronization, Pod listing, and Pod-content listing.
- [ ] The acceptance scenario no longer indexes MCP envelope or structured-content JSON directly.
- [ ] Wire-format tests may still inspect raw JSON where the wire shape itself is the behavior.
- [ ] The support module adds leverage rather than identity wrappers.

## Comments

- Added after the final combined thermo-nuclear review.
