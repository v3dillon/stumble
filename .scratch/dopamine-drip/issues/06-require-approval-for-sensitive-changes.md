# Require approval for sensitive changes

Status: complete

Blocked by: Authorize Agent Harnesses with scoped grants; Create and exchange portable Pod Packages

## Acceptance

- [x] Sensitive operations create a Pending Proposal rather than applying immediately.
- [x] Proposals state the requested change, affected resources, proposer, expiry, and expected consequences.
- [x] An interactive harness with approval permission can approve or reject a proposal.
- [x] An unattended harness cannot approve a proposal it created or expand its own grant.
- [x] Expired, rejected, and accepted proposals remain auditable.
- [x] Routine Feed, feedback, synchronization, Candidate Submission, and already-authorized curation operations remain one step.
- [x] Approval behavior is consistent across HTTP, MCP, and CLI.
