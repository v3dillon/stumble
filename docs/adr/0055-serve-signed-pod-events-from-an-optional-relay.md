# Serve signed Pod Events from an optional Relay

Status: Accepted

Relay is a third independent process capability next to Bootstrap and Index: one `stumble-api` process can enable any combination with `--bootstrap`, `--index`, and `--relay`. A Home Node with no public address pushes its Origin-signed Pod snapshot to a Relay, which stores and serves it unchanged at the public URL `/relay/pods/<origin_node_id>/<slug>`. Origin identity stays in the snapshot: subscribers pin `snapshot.node` as the Origin, verify every event with the Origin public key, and never treat the Relay host as the Origin. The Relay never re-signs, holds no Home Node private state, and is never mandatory (ADR-0031).
