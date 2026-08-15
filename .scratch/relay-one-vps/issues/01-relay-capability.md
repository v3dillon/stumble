# Relay capability on one VPS

Status: ready-for-agent

Track the Relay Node work from `PRD.md` in this directory.

Done in this change set:

- `RelayCapability` flag on `AgentTools` (`with_relay_capability`, `relay_enabled`), `--relay` / `STUMBLE_RELAY` on `stumble-api`, role log line.
- Canonical Relay URL shape `/relay/pods/<origin_node_id>/<slug>` in `canonical_public_pod_url` / `validate_public_pod_url`, plus `relay_public_pod_url_parts`.
- Relay HTTP surface (POST admit, GET snapshot/manifest/events) with the Bootstrap-style `relay_disabled` 404 pattern; well-known `relay_publications` and `relay_pod_snapshot_template` only while enabled.
- `admit_relay_snapshot` with Origin signature/chain verification, idempotent extension-only upsert, and the `relay_publications` SQLite collection.
- Relay-aware `fetch_pod_snapshot`, `ReqwestOriginProbe`, and the local Relay probe short-circuit for combined Bootstrap+Relay processes.
- `stumble pod publish --via-relay`, `stumble pod relay-push`, `stumble pod announce` Relay re-push, `submit_pod_snapshot_to_relay` client.
- ADR-0055, deploy script `--no-relay`, operator/user/deploy/README/discovery doc updates, acceptance tests.

Open follow-ups:

- Relay retention/expiry policy tied to the Announcement Lease.
- Relay-side payload bounds and per-origin quotas mirroring Bootstrap admission limits.
