# Discover and preview similar public Pods locally

Status: ready-for-agent
Blocked by: 03
Source: ../PRD.md

## What to build

Turn synchronized public announcements into useful local discovery. A Home Node should retrieve bounded Origin-signed Explore samples, calculate inspectable deterministic Pod Similarity, and give relevant endorsed or unendorsed public Pods tightly bounded exposure under the existing Trust Policy, Explore, and Feed rules.

## Acceptance criteria

- [ ] The Home Node retrieves bounded Explore samples directly from the canonical Origin and accepts them only when their signature and current announcement binding verify.
- [ ] Deterministic Pod Similarity uses verified public subject and Pod Context text, source neighborhoods, Explore samples, and valid Pod Endorsements.
- [ ] Similarity is calculated locally from synchronized metadata and private evidence without issuing background interest-derived remote queries.
- [ ] Explore results and Exploration Items provide inspectable reasons identifying subject, source, sample, or endorsement evidence.
- [ ] Pod Endorsements strengthen evidence but are neither mandatory nor treated as transferable trust or global reputation.
- [ ] A strongly relevant unendorsed Pod can receive limited labeled trial exposure after identity, reachability, manifest, announcement, and sample verification.
- [ ] Per-Origin, per-Pod, per-source, and existing Feed Mix exploration caps prevent open-admission flooding.
- [ ] Local Pod, Origin, source, and topic blocks exclude matching Pods and samples before ranking or delivery.
- [ ] Explicit Feedback Signals affect future local exposure while ignores and passive delivery do not create durable preference by themselves.
- [ ] Deterministic discovery remains functional with no active Agent Harness or model service.

## Comments

