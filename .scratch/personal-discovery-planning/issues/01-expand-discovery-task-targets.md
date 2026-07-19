Status: ready-for-agent
Blocked by: None

# Expand Discovery Tasks to typed targets

## Parent

Personal Discovery from User evidence PRD.

## What to build

Expand the Discovery Task model so its governing target is explicit and can represent either existing Pod discovery or future Personal Discovery. This is an expand-first compatibility refactor: all current Pod discovery behavior must remain observable and green while later tickets gain a clean non-Pod seam.

## Acceptance criteria

- [ ] Existing scheduled and immediate Pod Discovery Tasks use an explicit Pod target that retains Pod identity, Package version, Source Rule or conversational provenance, and lease history.
- [ ] Persisted pre-feature Discovery Tasks migrate without losing identity, provenance, state, attempts, or idempotency.
- [ ] Task materialization, listing, claim, renewal, completion, and failure expose the same behavior through supported adapters after the refactor.
- [ ] The target model can represent Personal Discovery without requiring a synthetic Pod or nullable Pod invariants.
- [ ] Existing Pod task authorization and Package pinning remain unchanged.
- [ ] Restart and adapter parity tests pass without introducing Personal Discovery behavior prematurely.

## Comments

Use an expand-first approach and keep the workspace green; contraction of obsolete representation is part of this ticket only when every maintained caller has migrated safely.
