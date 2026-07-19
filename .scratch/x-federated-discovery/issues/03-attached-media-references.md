Status: ready-for-agent

# Preserve attached media references with Content References

Blocked by: 01

Allow a Candidate Submission to carry typed permitted media URLs and preserve them on the accepted Content Reference and across signed Pod Event synchronization, so a post and its images remain one item.

- [ ] Candidate Submissions accept zero or more typed media references and remain backward-compatible when omitted.
- [ ] URL and type validation occur in the deterministic core.
- [ ] Accepted Content References expose attached media without claiming binary archival.
- [ ] Signed event export/import preserves identical media references.
- [ ] Tests cover submission, acceptance, deduplication behavior, and subscriber projection.

## Comments

