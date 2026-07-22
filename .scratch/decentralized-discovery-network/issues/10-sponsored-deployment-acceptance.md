# Prove and document the sponsored decentralized deployment

Status: ready-for-agent
Blocked by: 05, 07, 08, 09
Source: ../PRD.md

## What to build

Prove the complete decentralized discovery milestone through public multi-node behavior and provide the operator and User documentation needed to run, replace, disable, and reason about the sponsored Bootstrap and Index deployment.

## Acceptance criteria

- [x] A deterministic acceptance scenario runs separate Origin, sponsored Bootstrap/Index, Discovery Peer, and fresh Home Node instances against real temporary SQLite stores through public HTTP contracts.
  - Evidence: `crates/stumble-api/tests/sponsored_deployment_acceptance.rs` (`sponsored_multi_node_publish_sync_peer_outage_and_subscribe`)
- [x] The scenario publishes, admits, cursor-synchronizes, locally matches, explains, previews, and subscribes to a public Pod without private evidence reaching the sponsor.
  - Evidence: same principal test; Taste Profile set on Home; sponsor store assert; Explore reasons; explore samples; `subscribe_pod_from_url`
- [x] After the Home Node learns a Discovery Peer, the scenario makes the sponsor unavailable and proves continued receipt of a new valid announcement through peer exchange.
  - Evidence: learn with seeded selection + `ReqwestPeerAdvertisementSampleClient`; drop sponsor server; peer open-admit second Pod; `sync_outbound_discovery_peers`
- [x] The scenario covers renewal, expiry, signed withdrawal, malformed signature rejection, incompatible protocol, local blocking, restart recovery, cursor idempotency, peer eviction, and direct Pod URL fallback.
  - Evidence: principal test (restart, cursor idempotency, direct URL under outage) + `multi_node_renewal_withdrawal_rejections_block_index_policy_and_eviction`
- [x] The scenario proves an unendorsed relevant Pod can receive only bounded labeled trial exposure and a remote Index score cannot override local Trust Policy.
  - Evidence: trial_exposure + reasons in principal test; BlockPod after Index import in lifecycle test
- [x] Existing Agent Harness HTTP behavior proves browser-originated Candidates remain in finite Discovery Result Batches and never enter Feed exploration without explicit User action.
  - Evidence: `browser_candidates_remain_in_result_batches_not_feed`
- [x] Runtime configuration can independently enable Bootstrap and Index capabilities while Relay capability remains absent from the milestone.
  - Evidence: `runtime_enables_bootstrap_and_index_independently_without_relay`; well-known asserts in principal test
- [x] User documentation explains the sponsored default, multiple replacement Bootstraps, outbound-only Home Node default, serving opt-in, direct-address fallback, and behavior during sponsor outages.
  - Evidence: `docs/sponsored-bootstrap-users.md`
- [x] Operator documentation publishes open-admission verification, narrow moderation, rejection reasons, rate limits, no-account behavior, minimized security logging, configurable retention, and no product analytics or global ranking.
  - Evidence: `docs/sponsored-bootstrap-operators.md`
- [x] Full workspace formatting, compilation, lint, migration, and test checks pass.
  - Evidence: see agent return writeup commands

## Comments

- Multi-node HTTP acceptance uses real loopback listeners, temporary SQLite per node, deterministic announcement clocks (`pod_announcement_at` / `admit_*_at`), and seeded peer selection.
- Reqwest bootstrap/peer/index clients run inside `spawn_blocking` to avoid nested Tokio runtimes in async tests.
- No commit performed by the implementing agent.
