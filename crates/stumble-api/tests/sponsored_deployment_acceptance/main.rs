//! Multi-node HTTP acceptance for the sponsored decentralized deployment.
//!
//! Spins up separate Origin, sponsored Bootstrap/Index, Discovery Peer, and fresh
//! Home Node processes against real temporary SQLite stores and exercises public
//! HTTP contracts. Deterministic clocks (`pod_announcement_at`, explicit `now`)
//! and seeded peer selection keep the scenario reliable without wall-clock sleep.

mod capability_surfaces;
mod common;
mod multi_node;
