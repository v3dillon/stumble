//! Agent-facing tools organized by CLI family: node, pod, discover, feed, sync.

mod prelude;
mod shared;

mod discover;
mod feed;
mod node;
mod pod;
mod search;
mod sync;

#[cfg(test)]
mod tests;

pub(crate) use discover::*;
pub(crate) use feed::*;
pub(crate) use pod::*;
pub use search::{SearchHit, SearchRequest, SearchResults, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT};
pub use shared::canonicalize_url;
pub(crate) use shared::*;
pub use sync::canonical_public_pod_url;
pub(crate) use sync::*;

use prelude::*;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AgentToolsError {
    /// The Explore request's bounds are invalid.
    #[error(transparent)]
    ExploreRequest(#[from] ExploreRequestError),
    /// Origin Explore sample fetch or verification failed.
    #[error(transparent)]
    SampleFetch(#[from] SampleFetchError),
    #[error(transparent)]
    CurationRationale(#[from] CurationRationaleError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Signing(#[from] SigningError),
    #[error(transparent)]
    Persistence(#[from] StorePersistenceError),
    #[error("lock poisoned")]
    LockPoisoned,
    #[error("bad url: {0}")]
    BadUrl(String),
    #[error("harness authorization denied: {reason}")]
    Forbidden { reason: String },
    #[error("Discovery Task lease is held by another harness")]
    TaskLeaseConflict,
    #[error("Discovery Task is terminal")]
    TaskTerminal,
    #[error("Discovery Task has no active lease owned by this harness")]
    TaskLeaseRequired,
    #[error("candidate submission requires an authenticated Agent Harness")]
    CandidateHarnessRequired,
    #[error("unattended candidate submission requires a Discovery Task")]
    CandidateTaskRequired,
    #[error("candidate submission requires the submitting harness to own the active task lease")]
    CandidateTaskLeaseRequired,
    #[error("candidate submission Pod Package version does not match its Discovery Task")]
    CandidatePackageVersionMismatch,
    #[error("candidate submission idempotency key was reused with different input")]
    CandidateIdempotencyConflict,
    #[error("Personal Discovery needs an explicit interest, corroborated User evidence, or temporary intent")]
    PersonalDiscoveryNotReady,
    #[error("Personal Discovery idempotency key was reused with different input")]
    PersonalDiscoveryIdempotencyConflict,
    #[error("Home Node is not initialized")]
    NodeNotInitialized,
    #[error("Home Node is already initialized")]
    NodeAlreadyInitialized,
    /// A remote node advertises a protocol this node cannot safely interpret.
    #[error("incompatible protocol version {received}; this node supports {supported}")]
    IncompatibleProtocol {
        /// Protocol version advertised by the remote node.
        received: String,
        /// Protocol version supported by this node.
        supported: &'static str,
    },
    /// Open Bootstrap admission or stream request was rejected.
    #[error("bootstrap rejected: {reason}")]
    BootstrapRejected {
        /// Stable machine-readable rejection reason.
        reason: BootstrapAdmissionRejectionReason,
        /// Human-readable detail for operators and Origins.
        message: String,
    },
    /// Public Index search failed with a bounded typed outcome.
    #[error(transparent)]
    IndexSearch(#[from] IndexSearchFailure),
    /// The signed Pod Event Relay capability is not enabled on this process.
    #[error("relay is not enabled on this node")]
    RelayDisabled,
    /// The pushed snapshot or samples exceed the bounded size open Relay admission accepts.
    #[error("relay payload exceeds the bounded size")]
    RelayPayloadTooLarge,
    /// Discovery Peer enablement, admission, or serving was rejected.
    #[error("discovery peer rejected: {reason}")]
    DiscoveryPeerRejected {
        /// Stable machine-readable rejection reason.
        reason: DiscoveryPeerAdmissionRejectionReason,
        /// Human-readable detail for operators.
        message: String,
    },
}

pub(crate) const MAX_DISCOVERY_TASK_ATTEMPTS: usize = 3;
pub(crate) const DEFAULT_PENDING_PROPOSAL_SECONDS: u64 = 3_600;

#[derive(Clone)]
pub struct AgentTools {
    store: Arc<RwLock<InMemoryStore>>,
    persistence: Option<Persistence>,
    /// Independent Bootstrap capability configuration.
    bootstrap: BootstrapCapability,
    /// Independent Index capability (may share a process with Bootstrap).
    index: IndexCapability,
    /// Independent Relay capability (may share a process with Bootstrap/Index).
    relay: RelayCapability,
    /// Injectable reachability probe for Discovery Peer enablement and admission.
    discovery_peer_probe: Arc<dyn DiscoveryPeerProbe>,
}

/// Runtime configuration for open Bootstrap admission and Announcement Streams.
#[derive(Clone)]
struct BootstrapCapability {
    enabled: bool,
    origin_probe: Arc<dyn OriginProbe>,
}

impl Default for BootstrapCapability {
    fn default() -> Self {
        Self {
            enabled: false,
            origin_probe: Arc::new(UnreachableOriginProbe),
        }
    }
}

/// Runtime configuration for public Index announcement search.
#[derive(Clone)]
struct IndexCapability {
    enabled: bool,
}

impl Default for IndexCapability {
    fn default() -> Self {
        Self { enabled: false }
    }
}

/// Runtime configuration for the optional signed Pod Event Relay role.
#[derive(Clone)]
struct RelayCapability {
    enabled: bool,
}

impl Default for RelayCapability {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Clone)]
enum Persistence {
    Sqlite {
        path: Arc<PathBuf>,
        /// Record snapshot from the last successful persist or load; the
        /// change diff runs against this instead of a full store clone.
        baseline: Arc<Mutex<StoreRecords>>,
        /// Last store generation this process observed on disk.
        generation: Arc<std::sync::atomic::AtomicI64>,
    },
}
