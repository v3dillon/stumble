//! Outbound Home Node Discovery Peer rotation and Bootstrap-outage survival.
//!
//! Home Nodes learn signed peer advertisements from Bootstrap Nodes and existing
//! Discovery Peers, select a small rotating outbound set without granting Trusted
//! Peer status, and synchronize only Origin-signed public announcement lifecycle
//! artifacts. Network I/O is separated from store mutation so callers avoid
//! holding store locks across HTTP.

mod clients;
mod gossip;
mod sync;

pub use clients::*;
pub use gossip::*;
pub use sync::*;

#[cfg(test)]
mod tests;
