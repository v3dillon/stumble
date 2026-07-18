# Separate Subscription from Pod Role state

Status: complete

Blocked by: 01

Perform the domain and persistence transition that makes Feed eligibility independent from Pod governance while preserving existing User state.

## Acceptance criteria

- [x] Subscription is stored and evaluated independently from Pod authority.
- [x] Pod Role has only Owner and Curator variants.
- [x] Priority remains a Subscription property.
- [x] Existing owner and curator-equivalent authority migrates without loss.
- [x] Existing passive membership maps to Subscription only when it represented Feed eligibility.
- [x] A curator can manage a Pod without subscribing, and a subscriber receives no curation authority.
- [x] Persistence restart and legacy snapshot migration tests prove the transition is lossless and idempotent.

## Comments

- 2026-07-18: Implementation started after issue 01 completed and the repository passed the issue 04 full validation checkpoint.
- 2026-07-18: Separated Feed eligibility and Priority into persisted Subscriptions, reduced Pod Roles to Owner and Curator, and required a qualifying Pod Role alongside PodCuration Harness capability.
- 2026-07-18: Legacy JSON snapshots and pre-transition SQLite rows now migrate Owner, Moderator/Admin, Member, and Priority state deterministically; migration rewrites old SQLite rows once and remains stable across restart.
- 2026-07-18: Focused validation passed for the new relationship seam (3 tests), Feed Batches (16), Subscriptions (11), legacy JSON migration (1), Placement Tombstones (2), and Taste Profiles (8); `cargo test -p stumble-core` passed before final workspace validation.
- 2026-07-18: Review corrected one-time migration of pre-transition SQLite relationship rows and made cross-adapter and first-release fixtures subscribe explicitly; final `cargo fmt --check` and `cargo test --workspace` passed.
