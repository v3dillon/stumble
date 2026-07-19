Status: ready-for-agent
Blocked by: None

# Learn Interest Seeds from User submissions

## Parent

Personal Discovery from User evidence PRD.

## What to build

Turn authenticated User-submitted Content References into weak, private, retractable Interest Seeds and aggregate inspectable Source Affinities without allowing agent discoveries, retries, or duplicate URLs to train the profile.

## Acceptance criteria

- [ ] A canonical URL submitted through an authenticated interactive User action creates at most one Interest Seed for that User.
- [ ] A submission can explicitly disable learning while retaining the Candidate or Content Reference.
- [ ] The User can retract one seed's contribution later without deleting the saved reference or unrelated evidence.
- [ ] Acquisition origin is typed and enforced from authorization and task context; an unattended worker cannot label its own discovery as User-submitted evidence.
- [ ] Interest Seed enrichment preserves permitted provenance and supports generic topic, domain, publisher, author or account, community, and referrer-context evidence.
- [ ] One weak action remains inspectable with zero discovery weight until a second independent User action corroborates the same signal.
- [ ] Duplicate canonical URLs, retries, agent discoveries, views, silence, and batch delivery do not corroborate evidence.
- [ ] Explicit interests and blocks retain precedence; negative Feedback Signals oppose or block inferred affinities according to existing Taste Profile rules.
- [ ] The Taste Profile exposes aggregate evidence and Source Affinities without reconstructing raw URL history.
- [ ] Seeds and affinities persist across restart, migrate from pre-feature stores, and remain absent from every federation surface.
- [ ] Supported adapters expose equivalent submission controls, retraction, profile inspection, authorization errors, and idempotency.

## Comments

Follow ADR-0035. Agent-found content must never create a positive learning loop without a later explicit User action.
