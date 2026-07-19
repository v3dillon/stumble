Status: ready-for-agent

# Split media-enrichment synchronization tests

Blocked by: 06

Move the cohesive media-enrichment, restart, conflict, and cross-Pod ordering scenarios out of the general Subscription test target into a focused integration-test target with shared fixtures.

- [ ] The general Subscription test file returns below 1,000 lines.
- [ ] All media enrichment and ordering assertions remain intact.
- [ ] Shared fixtures are reused rather than copied.
- [ ] No production behavior changes.

## Comments

- Added after the final combined thermo-nuclear review.
