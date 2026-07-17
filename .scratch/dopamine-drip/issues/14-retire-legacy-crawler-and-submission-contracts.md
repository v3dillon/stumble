# Retire legacy crawler and submission contracts

Status: complete

Blocked by: 04, 07, 13

Complete the expand-migrate-contract transition from crawler, dedicated Source
Connector, legacy submission, and brief-centered behavior to Candidate, Content
Item, Pod Package, Discovery Task, and Feed Batch contracts.

## Acceptance

- [x] First-release workflows and adapter catalogs use canonical domain vocabulary.
- [x] Legacy persisted canonical identity, placements, feedback, and events migrate intact.
- [x] Retired operations are absent or return an explicit versioned compatibility error.
- [x] Brief behavior is documented as Agent Harness presentation of Feed Batches.
- [x] HTTP, MCP, and CLI expose equivalent IDs, provenance, errors, and allowed actions.
- [x] Protocol negotiation rejects incompatible event-shape versions before projection.

## Comments

- 2026-07-17: Implementation started with strict red-green TDD at the persisted-store, adapter-catalog, compatibility-error, and federation-negotiation seams.
- 2026-07-17: Completed with lossless legacy import coverage, canonical public catalogs, explicit versioned retirement errors, and pre-projection protocol negotiation.
- 2026-07-17: Standards and Spec review findings were corrected: compatibility mappings are core-owned, hub and peer sync negotiate before event fetch/import, peer sync has typed tested failures, and every advertised sync operation performs real work or returns an explicit retirement error.
