Status: ready-for-agent

# Isolate peer synchronization blocking work

Blocked by: 05

Make the trusted-peer synchronization workflow follow the same blocking-read, asynchronous-fetch, blocking-apply structure as direct-Origin synchronization.

- [ ] Subscription lookup never runs directly on a Tokio worker.
- [ ] Snapshot projection and SQLite persistence never run directly on a Tokio worker.
- [ ] Network fetching remains asynchronous.
- [ ] Existing peer verification and error behavior remain unchanged.

## Comments

- Added after the final combined thermo-nuclear review.
