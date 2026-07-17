# Submit provenance-bearing Candidates

Status: complete

Blocked by: Authorize Agent Harnesses with scoped grants; Create and exchange portable Pod Packages; Claim scheduled Discovery Tasks

## Acceptance

- [x] Candidate Submission accepts source URL, known source metadata, permitted excerpt and summary, content type, tags, provenance, and placement evidence.
- [x] Task-driven submissions carry the task and Pod Package versions used during discovery.
- [x] Harness and client idempotency keys make retries safe.
- [x] Canonical identity deduplicates repeated discoveries without losing independent placement evidence.
- [x] One submission may propose several authorized local Pod Placements with separate reasons and confidence.
- [x] Harness confidence is retained as evidence but does not directly create authoritative placements.
- [x] Candidates and review state never appear in federation exports.
- [x] HTTP, MCP, and CLI expose equivalent submission and inspection behavior.
