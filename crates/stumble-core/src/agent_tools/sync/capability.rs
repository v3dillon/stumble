use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Enables or reconfigures the independent Bootstrap capability.
    ///
    /// Bootstrap does not grant Origin proxy authority or private-state access.
    /// The injected [`OriginProbe`] verifies Origin reachability and public manifests.
    #[must_use]
    pub fn with_bootstrap_capability(
        mut self,
        enabled: bool,
        origin_probe: Arc<dyn OriginProbe>,
    ) -> Self {
        self.bootstrap = BootstrapCapability {
            enabled,
            origin_probe,
        };
        self
    }

    /// Enables or disables the independent Index capability.
    ///
    /// Index may share a process with Bootstrap. Enabling Index does not grant
    /// ranking or trust authority over Home Nodes.
    #[must_use]
    pub fn with_index_capability(mut self, enabled: bool) -> Self {
        self.index = IndexCapability { enabled };
        self
    }

    /// Returns whether this process currently enables open Bootstrap admission.
    #[must_use]
    pub fn bootstrap_enabled(&self) -> bool {
        self.bootstrap.enabled
    }

    /// Returns whether this process currently enables public Index search.
    #[must_use]
    pub fn index_enabled(&self) -> bool {
        self.index.enabled
    }

    /// Injects the reachability probe used for Discovery Peer enablement and
    /// Bootstrap peer-advertisement admission.
    #[must_use]
    pub fn with_discovery_peer_probe(mut self, probe: Arc<dyn DiscoveryPeerProbe>) -> Self {
        self.discovery_peer_probe = probe;
        self
    }

    /// Returns whether this Home Node currently enables inbound Discovery Peer serving.
    ///
    /// Default is false: ordinary Home Nodes remain outbound-only for discovery.
    #[must_use]
    pub fn discovery_peer_service_enabled(&self) -> bool {
        self.store
            .read()
            .map(|store| peer_service_is_enabled(&store))
            .unwrap_or(false)
    }

}
