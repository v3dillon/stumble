use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Requests a Trust Policy addition without applying it immediately.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, validation, or persistence fails.
    pub fn request_add_trusted_peer(
        &self,
        ctx: &AuthContext,
        display_name: String,
        base_url: String,
        public_key: String,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::AddTrustedPeer {
                    node_id: Uuid::nil(),
                    display_name,
                    base_url,
                    public_key,
                },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Requests approval to trust one canonical remote Node identity.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, identity validation, or persistence fails.
    pub fn request_add_trusted_node(
        &self,
        ctx: &AuthContext,
        node: NodeInfo,
        base_url: String,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        if node.node_id.is_nil() {
            return Err(StoreError::Validation("canonical Node ID must not be nil".into()).into());
        }
        if node.supported_protocol_version != CURRENT_PROTOCOL_VERSION {
            return Err(StoreError::Validation(format!(
                "unsupported Node protocol {}",
                node.supported_protocol_version
            ))
            .into());
        }
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::AddTrustedPeer {
                    node_id: node.node_id,
                    display_name: node.display_name,
                    base_url,
                    public_key: node.public_key,
                },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Changes the local Trust Policy: the direct Home Node Owner applies
    /// immediately (they are the approval authority — ADR-0033); an Agent
    /// Harness receives a Pending Proposal for independent approval.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization, validation, or persistence fails.
    pub fn change_trust_policy(
        &self,
        ctx: &AuthContext,
        change: TrustPolicyChange,
        now: chrono::DateTime<Utc>,
    ) -> Result<TrustPolicyChangeOutcome, AgentToolsError> {
        if ctx.harness_id.is_some() {
            return self
                .request_trust_policy_change(ctx, change, now)
                .map(|proposal| TrustPolicyChangeOutcome::PendingApproval(Box::new(proposal)));
        }
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Trust Policy changes require an authenticated User".into())
        })?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let key = (user_id, ctx.tenant_id);
        let mut policy = store
            .trust_policies
            .get(&key)
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
        apply_trust_policy_change(&mut policy, &change)?;
        store.trust_policies.insert(key, policy.clone());
        self.persist_locked(&mut store)?;
        Ok(TrustPolicyChangeOutcome::Applied(Box::new(policy)))
    }

    /// Requests an independently approved local Trust Policy change.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, validation, or persistence fails.
    pub fn request_trust_policy_change(
        &self,
        ctx: &AuthContext,
        change: TrustPolicyChange,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::ChangeTrustPolicy { change },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Requests independent approval to disable one trusted peer.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, validation, or persistence fails.
    pub fn request_remove_trusted_peer(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::RemoveTrustedPeer { peer_id },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Returns the authenticated User's local public discovery Trust Policy.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// or local state is unavailable.
    pub fn trust_policy(&self, ctx: &AuthContext) -> Result<TrustPolicy, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Trust Policy requires an authenticated User".into())
        })?;
        Ok(store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id)))
    }

    pub fn well_known_node(
        &self,
        ctx: &AuthContext,
        base_url: &str,
    ) -> Result<WellKnownNode, AgentToolsError> {
        let node = self.node_info(ctx)?;
        let base = base_url.trim_end_matches('/');
        let mut endpoints = BTreeMap::new();
        endpoints.insert("node".to_string(), format!("{base}/federation/node"));
        endpoints.insert("pods".to_string(), format!("{base}/federation/pods"));
        endpoints.insert(
            "pod_manifest_template".to_string(),
            format!("{base}/federation/pods/{{slug}}/manifest"),
        );
        endpoints.insert(
            "pod_events_template".to_string(),
            format!("{base}/federation/pods/{{slug}}/events"),
        );
        if self.bootstrap.enabled {
            endpoints.insert(
                "bootstrap_announcements".to_string(),
                format!("{base}/bootstrap/announcements"),
            );
            endpoints.insert(
                "bootstrap_announcement_stream".to_string(),
                format!("{base}/bootstrap/announcements/stream"),
            );
            endpoints.insert(
                "bootstrap_withdrawals".to_string(),
                format!("{base}/bootstrap/withdrawals"),
            );
        }
        if self.index.enabled {
            endpoints.insert(
                "index_search_announcements".to_string(),
                format!("{base}/discovery/announcements"),
            );
        }
        // Relay endpoints are advertised only while the independent Relay
        // capability is on; a Bootstrap/Index-only process never mentions Relay.
        if self.relay.enabled {
            endpoints.insert(
                "relay_publications".to_string(),
                format!("{base}/relay/pods/{{origin_node_id}}/{{slug}}"),
            );
            endpoints.insert(
                "relay_pod_snapshot_template".to_string(),
                format!("{base}/relay/pods/{{origin_node_id}}/{{slug}}"),
            );
            endpoints.insert(
                "relay_explore_samples_template".to_string(),
                format!("{base}/relay/pods/{{origin_node_id}}/{{slug}}/explore-samples"),
            );
        }
        // Discovery Peer inbound endpoints are advertised only while the User has
        // explicitly opted into announcement serving (ADR-0044 / ADR-0049).
        if self.discovery_peer_service_enabled() {
            endpoints.insert(
                "discovery_peer_announcement_stream".to_string(),
                format!("{base}/discovery/peer/announcements/stream"),
            );
            endpoints.insert(
                "discovery_peer_advertisement_sample".to_string(),
                format!("{base}/discovery/peer/advertisements"),
            );
        }
        if self.bootstrap.enabled {
            endpoints.insert(
                "bootstrap_peer_advertisements".to_string(),
                format!("{base}/bootstrap/peer-advertisements"),
            );
            endpoints.insert(
                "bootstrap_peer_advertisement_sample".to_string(),
                format!("{base}/bootstrap/peer-advertisements"),
            );
        }
        Ok(WellKnownNode {
            protocol: CURRENT_PROTOCOL_VERSION.to_string(),
            node,
            endpoints,
        })
    }

    /// Lists peers explicitly enabled by this Home Node's local Trust Policy.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied or state is unavailable.
    pub fn trusted_peers(&self, ctx: &AuthContext) -> Result<Vec<TrustedPeer>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let mut peers = store
            .trusted_peers
            .values()
            .filter(|peer| peer.tenant_id == ctx.tenant_id && peer.enabled)
            .cloned()
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| left.base_url.cmp(&right.base_url));
        Ok(peers)
    }

    /// Resolves one enabled peer within the caller's tenant trust boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the peer is absent,
    /// disabled, belongs to another tenant, or state is unavailable.
    pub fn trusted_peer(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
    ) -> Result<TrustedPeer, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        store
            .trusted_peers
            .get(&peer_id)
            .filter(|peer| peer.tenant_id == ctx.tenant_id && peer.enabled)
            .cloned()
            .ok_or_else(|| StoreError::UntrustedPeer.into())
    }
}
