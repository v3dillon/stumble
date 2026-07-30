//! First-release integration proof for the `first_release` test target.
//!
//! The scenario helpers live in `tests/first_release/`: `common` holds
//! node/harness fixtures and adapter assertions, `scenario` holds the
//! two-node scenario flows, and `release_proof` holds the `#[test]`.

mod common;
mod release_proof;
mod scenario;
