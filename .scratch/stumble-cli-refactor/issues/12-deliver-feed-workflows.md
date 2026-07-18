# Deliver Feed workflows

Status: complete

Blocked by: 06, 08, 11

Expose stable Feed Batch consumption, explicit Feedback Signals, and private Taste Profile management through the canonical Feed family.

## Acceptance criteria

- [x] `feed batch get` returns the current stable Feed Batch through the versioned machine contract.
- [x] Repeated retrieval returns the same batch until `feed batch complete` reaches Caught Up.
- [x] Batch Intent and Feed Mix inputs use the shared structured-input contract.
- [x] `feed feedback record` handles canonical Feedback Signals, including item-driven source and topic blocks.
- [x] `feed taste show`, `set`, and `reset` expose explicit preferences and inspectable learned weights without federating private state.
- [x] Standalone block commands and Drip parser aliases are absent.
- [x] Feed results include explanations, canonical placement evidence, and allowed actions.

## Comments

- 2026-07-18: Implementation started after issues 06, 08, and 11 completed and the corrected issue 11 checkpoint passed full workspace validation.
- 2026-07-18: Delivered the canonical Feed CLI workflows at the real executable seam. Batch retrieval accepts structured Feed Mix and Batch Intent input, preserves one stable batch until explicit completion, and returns ranking explanations plus Pod ID/slug placement evidence and state-sensitive actions. Unified Feedback Signals now handle source and item-topic blocks, and Taste Profile show/set/reset keeps explicit and learned state private and inspectable. Focused executable, Feed Batch, Taste Profile, and full CLI package tests pass; self-review additionally constrained item-driven topic blocks to topics carried by the Delivered Item.
