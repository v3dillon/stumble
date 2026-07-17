# Claim scheduled Discovery Tasks

Status: complete

Blocked by: Authorize Agent Harnesses with scoped grants; Create and exchange portable Pod Packages

## Acceptance

- [x] Due Source Rules create idempotent, version-pinned Discovery Tasks.
- [x] Authorized harnesses can list, claim, renew, complete, and fail tasks.
- [x] Expiring leases prevent duplicate execution and abandoned attempts remain inspectable.
- [x] Failure and lease-expiry retries reach an inspectable terminal state.
- [x] A launchd/cron-friendly adapter wakes due work through the running Home Node.
- [x] The adapter emits a durable Discovery-ready Event or invokes an explicit harness command without browser control.
- [x] Conversational discovery creates an intent-bearing, idempotent immediate task through the same lifecycle.
