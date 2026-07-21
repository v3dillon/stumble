# Prove and document the sponsored decentralized deployment

Status: ready-for-agent
Blocked by: 05, 07, 08, 09
Source: ../PRD.md

## What to build

Prove the complete decentralized discovery milestone through public multi-node behavior and provide the operator and User documentation needed to run, replace, disable, and reason about the sponsored Bootstrap and Index deployment.

## Acceptance criteria

- [ ] A deterministic acceptance scenario runs separate Origin, sponsored Bootstrap/Index, Discovery Peer, and fresh Home Node instances against real temporary SQLite stores through public HTTP contracts.
- [ ] The scenario publishes, admits, cursor-synchronizes, locally matches, explains, previews, and subscribes to a public Pod without private evidence reaching the sponsor.
- [ ] After the Home Node learns a Discovery Peer, the scenario makes the sponsor unavailable and proves continued receipt of a new valid announcement through peer exchange.
- [ ] The scenario covers renewal, expiry, signed withdrawal, malformed signature rejection, incompatible protocol, local blocking, restart recovery, cursor idempotency, peer eviction, and direct Pod URL fallback.
- [ ] The scenario proves an unendorsed relevant Pod can receive only bounded labeled trial exposure and a remote Index score cannot override local Trust Policy.
- [ ] Existing Agent Harness HTTP behavior proves browser-originated Candidates remain in finite Discovery Result Batches and never enter Feed exploration without explicit User action.
- [ ] Runtime configuration can independently enable Bootstrap and Index capabilities while Relay capability remains absent from the milestone.
- [ ] User documentation explains the sponsored default, multiple replacement Bootstraps, outbound-only Home Node default, serving opt-in, direct-address fallback, and behavior during sponsor outages.
- [ ] Operator documentation publishes open-admission verification, narrow moderation, rejection reasons, rate limits, no-account behavior, minimized security logging, configurable retention, and no product analytics or global ranking.
- [ ] Full workspace formatting, compilation, lint, migration, and test checks pass.

## Comments

