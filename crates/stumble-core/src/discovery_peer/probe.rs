//! Public endpoint reachability probe used by Discovery Peer enablement and
//! Bootstrap peer-advertisement admission.

use crate::domain::{
    DiscoveryPeerAdvertisement, DiscoveryPeerIdentityView, NodeIdentity, CURRENT_PROTOCOL_VERSION,
};

/// Failure while probing a Discovery Peer public endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryPeerProbeError {
    /// Transport or DNS failure; endpoint is not currently reachable.
    Unreachable,
}

/// Port for verifying that a declared Discovery Peer endpoint is reachable and
/// advertising a usable identity view (node id, public key, protocol).
///
/// Production nodes inject an HTTP client that fetches public discovery metadata
/// (for example well-known node metadata). Tests inject deterministic fakes.
pub trait DiscoveryPeerProbe: Send + Sync {
    /// Probes `public_endpoint` for reachability and returns the live identity view.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryPeerProbeError::Unreachable`] when the endpoint cannot
    /// be reached or does not expose a usable identity view.
    fn probe_peer_endpoint(
        &self,
        public_endpoint: &str,
    ) -> Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError>;
}

/// Builds a probe identity view that matches a local node identity.
#[must_use]
pub fn peer_identity_view_for_node(node: &NodeIdentity) -> DiscoveryPeerIdentityView {
    DiscoveryPeerIdentityView {
        node_id: node.id,
        public_key: node.public_key.clone(),
        protocol_version: CURRENT_PROTOCOL_VERSION.into(),
    }
}

/// Builds a probe identity view that matches a signed peer advertisement.
#[must_use]
pub fn peer_identity_view_for_advertisement(
    advertisement: &DiscoveryPeerAdvertisement,
) -> DiscoveryPeerIdentityView {
    DiscoveryPeerIdentityView {
        node_id: advertisement.node_id,
        public_key: advertisement.signer.public_key.clone(),
        protocol_version: advertisement.protocol_version.clone(),
    }
}

/// Probe that always reports the endpoint unreachable.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnreachableDiscoveryPeerProbe;

impl DiscoveryPeerProbe for UnreachableDiscoveryPeerProbe {
    fn probe_peer_endpoint(
        &self,
        _public_endpoint: &str,
    ) -> Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError> {
        Err(DiscoveryPeerProbeError::Unreachable)
    }
}

/// Configurable probe that returns a fixed identity view or a fixed error.
#[derive(Debug, Clone)]
pub struct FixedDiscoveryPeerProbe {
    /// Successful identity view when `error` is `None`.
    pub identity: Option<DiscoveryPeerIdentityView>,
    /// Forced probe failure when present.
    pub error: Option<DiscoveryPeerProbeError>,
}

impl FixedDiscoveryPeerProbe {
    /// Builds a probe that always reports reachability with `identity`.
    #[must_use]
    pub fn reachable(identity: DiscoveryPeerIdentityView) -> Self {
        Self {
            identity: Some(identity),
            error: None,
        }
    }

    /// Builds a probe that always reports reachability matching `node`.
    #[must_use]
    pub fn matching_node(node: &NodeIdentity) -> Self {
        Self::reachable(peer_identity_view_for_node(node))
    }

    /// Builds a probe that always reports reachability matching `advertisement`.
    #[must_use]
    pub fn matching_advertisement(advertisement: &DiscoveryPeerAdvertisement) -> Self {
        Self::reachable(peer_identity_view_for_advertisement(advertisement))
    }

    /// Builds a probe that always reports unreachability.
    #[must_use]
    pub const fn unreachable() -> Self {
        Self {
            identity: None,
            error: Some(DiscoveryPeerProbeError::Unreachable),
        }
    }
}

impl DiscoveryPeerProbe for FixedDiscoveryPeerProbe {
    fn probe_peer_endpoint(
        &self,
        _public_endpoint: &str,
    ) -> Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.identity
            .clone()
            .ok_or(DiscoveryPeerProbeError::Unreachable)
    }
}

/// Scripted probe that returns a sequence of identity outcomes (test double).
#[derive(Debug, Default)]
pub struct ScriptedDiscoveryPeerProbe {
    outcomes: std::sync::Mutex<Vec<Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError>>>,
}

impl ScriptedDiscoveryPeerProbe {
    /// Queues probe outcomes in FIFO order.
    pub fn push(&self, outcome: Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError>) {
        self.outcomes.lock().expect("probe lock").push(outcome);
    }
}

impl DiscoveryPeerProbe for ScriptedDiscoveryPeerProbe {
    fn probe_peer_endpoint(
        &self,
        _public_endpoint: &str,
    ) -> Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError> {
        let mut outcomes = self.outcomes.lock().expect("probe lock");
        if outcomes.is_empty() {
            return Err(DiscoveryPeerProbeError::Unreachable);
        }
        outcomes.remove(0)
    }
}

/// Probe that returns a replaceable identity view for any endpoint (tests).
///
/// Production-ish tests set the expected identity from the local node or from a
/// signed advertisement before enable/admit, then assert mismatch rejection by
/// swapping in a different view.
#[derive(Debug, Default)]
pub struct SimpleMatchingDiscoveryPeerProbe {
    identity: std::sync::Mutex<Option<DiscoveryPeerIdentityView>>,
}

impl SimpleMatchingDiscoveryPeerProbe {
    /// Builds a probe preloaded with `identity`.
    #[must_use]
    pub fn new(identity: DiscoveryPeerIdentityView) -> Self {
        Self {
            identity: std::sync::Mutex::new(Some(identity)),
        }
    }

    /// Builds a probe preloaded from a local node identity.
    #[must_use]
    pub fn from_node(node: &NodeIdentity) -> Self {
        Self::new(peer_identity_view_for_node(node))
    }

    /// Builds a probe preloaded from a signed advertisement.
    #[must_use]
    pub fn from_advertisement(advertisement: &DiscoveryPeerAdvertisement) -> Self {
        Self::new(peer_identity_view_for_advertisement(advertisement))
    }

    /// Replaces the identity view returned by subsequent probes.
    pub fn set_identity(&self, identity: DiscoveryPeerIdentityView) {
        *self.identity.lock().expect("probe lock") = Some(identity);
    }

    /// Clears the identity so subsequent probes report unreachability.
    pub fn clear(&self) {
        *self.identity.lock().expect("probe lock") = None;
    }
}

impl DiscoveryPeerProbe for SimpleMatchingDiscoveryPeerProbe {
    fn probe_peer_endpoint(
        &self,
        _public_endpoint: &str,
    ) -> Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError> {
        self.identity
            .lock()
            .expect("probe lock")
            .clone()
            .ok_or(DiscoveryPeerProbeError::Unreachable)
    }
}
