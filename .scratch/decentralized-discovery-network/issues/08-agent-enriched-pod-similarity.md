# Enrich Pod Similarity with authorized local agent evidence

Status: ready-for-agent
Blocked by: 04
Source: ../PRD.md

## What to build

Allow a local Node Agent or narrowly authorized Agent Harness to add richer semantic evidence to Pod Similarity while Core remains the sole authority over eligibility, provenance, Trust Policy, exploration caps, and durable User learning.

## Acceptance criteria

- [ ] A scoped capability permits submitting bounded, confidence-scored, evidence-backed semantic relationships between exact current Pod Announcements.
- [ ] Submissions identify the public inputs used and are rejected when announcements are stale, withdrawn, expired, blocked, mismatched, or unverifiable.
- [ ] Agent evidence can adjust local ordering and produce an inspectable explanation but cannot create trust, Subscription, Accepted Placement, or Feed eligibility by itself.
- [ ] Deterministic policy applies existing caps and blocks after agent evidence is considered.
- [ ] Agent evidence never leaves the Home Node as an Endorsement, global score, announcement field, or remote interest query.
- [ ] Revoking the Harness Grant immediately prevents new evidence and excludes evidence attributable only to that revoked grant from current ranking.
- [ ] Duplicate submissions are idempotent and bounded by Pod pair, model or harness provenance, and freshness.
- [ ] Local semantic evidence and its audit provenance survive SQLite restart.
- [ ] With no agent evidence or active harness, deterministic Pod Similarity produces the same externally observable baseline behavior as before.

## Comments

