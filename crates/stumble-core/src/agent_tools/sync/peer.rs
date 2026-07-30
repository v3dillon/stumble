use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Enables inbound Discovery Peer announcement serving after verification.
    ///
    /// Requires a declared public endpoint and successful identity, protocol,
    /// HTTPS-outside-loopback, and reachability checks. Produces a signed
    /// expiring Discovery Peer Advertisement.
    ///
    /// # Errors
    ///
    /// Returns authorization, verification, or persistence errors.
    pub fn enable_discovery_peer_service(
        &self,
        ctx: &AuthContext,
        public_endpoint: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let advertisement = enable_discovery_peer_service(
            &mut store,
            &node,
            public_endpoint,
            self.discovery_peer_probe.as_ref(),
            now,
        )
        .map_err(|reason| AgentToolsError::DiscoveryPeerRejected {
            message: format!("discovery peer enable rejected: {reason}"),
            reason,
        })?;
        self.persist_locked(&mut store)?;
        Ok(advertisement)
    }

    /// Disables inbound Discovery Peer serving without affecting outbound discovery.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors.
    pub fn disable_discovery_peer_service(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerServiceState, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        disable_discovery_peer_service(&mut store, now);
        let state = store.discovery_peer_service.clone().unwrap_or_default();
        self.persist_locked(&mut store)?;
        Ok(state)
    }

    /// Reports Discovery Peer opt-in state for operators.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn discovery_peer_service_status(
        &self,
        ctx: &AuthContext,
    ) -> Result<DiscoveryPeerServiceState, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(store.discovery_peer_service.clone().unwrap_or_default())
    }

    /// Renews the current Discovery Peer Advertisement while service is enabled.
    ///
    /// # Errors
    ///
    /// Returns authorization, verification, or persistence errors.
    pub fn renew_discovery_peer_advertisement(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let advertisement = renew_discovery_peer_advertisement(
            &mut store,
            &node,
            self.discovery_peer_probe.as_ref(),
            now,
        )
        .map_err(|reason| AgentToolsError::DiscoveryPeerRejected {
            message: format!("discovery peer renew rejected: {reason}"),
            reason,
        })?;
        self.persist_locked(&mut store)?;
        Ok(advertisement)
    }

    /// Open Bootstrap admission for a signed Discovery Peer Advertisement.
    ///
    /// Requires no User account or Trusted Peer relationship.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::DiscoveryPeerRejected`] on verification failure.
    pub fn admit_discovery_peer_advertisement(
        &self,
        advertisement: DiscoveryPeerAdvertisement,
    ) -> Result<DiscoveryPeerAdmissionAcceptance, AgentToolsError> {
        self.admit_discovery_peer_advertisement_at(advertisement, Utc::now())
    }

    /// Open Bootstrap peer-advertisement admission at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::admit_discovery_peer_advertisement`].
    pub fn admit_discovery_peer_advertisement_at(
        &self,
        advertisement: DiscoveryPeerAdvertisement,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdmissionAcceptance, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let result = admit_discovery_peer_advertisement(
            &mut store,
            advertisement,
            self.discovery_peer_probe.as_ref(),
            self.bootstrap.enabled,
            now,
        );
        self.persist_locked(&mut store)?;
        result.map_err(|reason| AgentToolsError::DiscoveryPeerRejected {
            message: format!("discovery peer admission rejected: {reason}"),
            reason,
        })
    }

    /// Serves a bounded Announcement Stream page from an enabled Discovery Peer.
    ///
    /// Preserves Origin announcement bytes and signatures unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::DiscoveryPeerRejected`] when service is disabled
    /// or the cursor is invalid.
    pub fn peer_announcement_stream(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        self.peer_announcement_stream_at(cursor, limit, Utc::now())
    }

    /// Peer Announcement Stream at an explicit clock time.
    ///
    /// Read-only: acquires a shared store lock and does not persist.
    ///
    /// # Errors
    ///
    /// Same as [`Self::peer_announcement_stream`].
    pub fn peer_announcement_stream_at(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
        now: chrono::DateTime<Utc>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        read_peer_announcement_stream(&store, cursor, limit, now).map_err(|reason| {
            AgentToolsError::DiscoveryPeerRejected {
                message: format!("discovery peer stream rejected: {reason}"),
                reason,
            }
        })
    }

    /// Serves a small randomized sample of current peer advertisements (unranked).
    ///
    /// Uses server entropy for shuffle selection. Deterministic sampling for
    /// tests is available via [`Self::peer_advertisement_sample_at`].
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::DiscoveryPeerRejected`] when service is disabled.
    pub fn peer_advertisement_sample(
        &self,
        limit: Option<usize>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        self.peer_advertisement_sample_at(limit, server_sample_seed(), Utc::now())
    }

    /// Peer advertisement sample at an explicit clock time and optional test seed.
    ///
    /// `seed` is for Core/tests only; production HTTP must not accept a client
    /// seed query parameter.
    ///
    /// # Errors
    ///
    /// Same as [`Self::peer_advertisement_sample`].
    pub fn peer_advertisement_sample_at(
        &self,
        limit: Option<usize>,
        seed: u64,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        sample_discovery_peer_advertisements(&store, limit, now, seed).map_err(|reason| {
            AgentToolsError::DiscoveryPeerRejected {
                message: format!("discovery peer sample rejected: {reason}"),
                reason,
            }
        })
    }

    /// Bootstrap-open sample of currently admitted peer advertisements (unranked).
    ///
    /// Available when Bootstrap capability is enabled. Does not require Discovery
    /// Peer serving opt-in. Uses server entropy for shuffle.
    ///
    /// # Errors
    ///
    /// Returns rejection when Bootstrap is disabled.
    pub fn bootstrap_peer_advertisement_sample(
        &self,
        limit: Option<usize>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        self.bootstrap_peer_advertisement_sample_at(limit, server_sample_seed(), Utc::now())
    }

    /// Bootstrap peer-advertisement sample at an explicit clock and seed.
    ///
    /// # Errors
    ///
    /// Returns rejection when Bootstrap is disabled.
    pub fn bootstrap_peer_advertisement_sample_at(
        &self,
        limit: Option<usize>,
        seed: u64,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        if !self.bootstrap.enabled {
            return Err(AgentToolsError::DiscoveryPeerRejected {
                message: "bootstrap peer sample rejected: bootstrap_disabled".into(),
                reason: DiscoveryPeerAdmissionRejectionReason::BootstrapDisabled,
            });
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(sample_known_discovery_peer_advertisements(
            &store, limit, now, seed,
        ))
    }

    /// Enables or disables automatic Discovery Peer gossip without deleting audit state.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors.
    pub fn set_automatic_peer_gossip_enabled(
        &self,
        ctx: &AuthContext,
        enabled: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerGossipConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let config = set_automatic_peer_gossip_enabled(&mut store, enabled, now);
        self.persist_locked(&mut store)?;
        Ok(config)
    }

    /// Reports automatic peer gossip configuration.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn peer_gossip_config(
        &self,
        ctx: &AuthContext,
    ) -> Result<DiscoveryPeerGossipConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(ensure_discovery_peer_gossip_config(&mut store).clone())
    }

    /// Lists the rotating outbound Discovery Peer set with sync/health state.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn outbound_discovery_peers(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<OutboundDiscoveryPeerStatus>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(outbound_discovery_peer_statuses(&store))
    }

    /// Reports discovery readiness (including degraded Bootstrap-outage state).
    ///
    /// Direct Pod URL operation remains available when discovery is degraded.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn discovery_status(&self, ctx: &AuthContext) -> Result<DiscoveryStatus, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(discovery_status(&store))
    }

    /// Learns peer advertisements from Bootstrap and existing peers, then selects
    /// a bounded outbound set. Selection is randomized under `selection_seed`
    /// (tests inject a fixed seed; production may use server entropy).
    ///
    /// Network sample fetches run **outside** the store write lock; verification,
    /// retention, and selection run under a short write lock. Does not create
    /// Trusted Peer relationships.
    ///
    /// Reachability probing is **not** applied on this path by default: outbound
    /// learning verifies the signed advertisement locally (identity, capability,
    /// protocol, endpoint policy, lease, signature). Live reachability remains
    /// required when enabling a node as a Discovery Peer. Operators that want
    /// live endpoint match during learn can call the core retain helper with an
    /// injected [`DiscoveryPeerProbe`].
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors. Per-source sample failures
    /// fall through without aborting the pass.
    pub fn learn_and_select_discovery_peers(
        &self,
        ctx: &AuthContext,
        sample_client: &dyn PeerAdvertisementSampleClient,
        now: chrono::DateTime<Utc>,
        selection_seed: u64,
    ) -> Result<crate::discovery_peer::PeerLearnReport, AgentToolsError> {
        let (mut sources, local_node_id, gossip_enabled) = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
            let mut sources: Vec<String> = store
                .bootstrap_endpoints
                .values()
                .filter(|endpoint| endpoint.enabled)
                .map(|endpoint| endpoint.base_url.clone())
                .collect();
            sources.extend(
                list_active_outbound_peers(&store)
                    .into_iter()
                    .map(|peer| peer.public_endpoint),
            );
            let local_node_id = store.default_node().ok().map(|node| node.id);
            let gossip_enabled = peer_gossip_is_enabled(&store);
            (sources, local_node_id, gossip_enabled)
        };

        if !gossip_enabled {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            return Ok(crate::discovery_peer::PeerLearnReport {
                selected: list_active_outbound_peers(&store),
                ..Default::default()
            });
        }

        sources.sort();
        sources.dedup();

        // Sample fetches without holding any store lock (no HTTP under write).
        let fetched =
            crate::discovery_peer::fetch_peer_advertisement_samples(sample_client, &sources);

        // Short write: verify/retain/select only (no network I/O).
        // Probe is None: signed-ad local verify is sufficient for outbound learning.
        // Default UnreachableDiscoveryPeerProbe would soft-skip every ad.
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let report = crate::discovery_peer::retain_learned_samples_and_select(
            &mut store,
            &fetched,
            None,
            local_node_id,
            now,
            selection_seed,
        );
        self.persist_locked(&mut store)?;
        Ok(report)
    }

    /// Synchronizes Announcement Streams from each viable outbound Discovery Peer.
    ///
    /// Network I/O runs outside the store write lock. Invalid data, flooding,
    /// incompatible versions, expired advertisements, or repeated transport
    /// failures cause bounded backoff and automatic local eviction.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors. Per-peer typed failures are
    /// reported inside the [`DiscoveryPeerSyncReport`].
    pub fn sync_outbound_discovery_peers(
        &self,
        ctx: &AuthContext,
        client: &dyn DiscoveryPeerStreamClient,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerSyncReport, AgentToolsError> {
        let plans = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
            plan_discovery_peer_sync(&store, now)
        };

        let mut outcomes = Vec::with_capacity(plans.len());
        let mut retained_announcements = 0usize;
        let mut retained_withdrawals = 0usize;
        let mut evicted = Vec::new();

        for plan in plans {
            let fetched = fetch_discovery_peer_stream_pages(
                client,
                &plan.peer.public_endpoint,
                plan.cursor.clone(),
            );
            let outcome = {
                let mut store = self
                    .store
                    .write()
                    .map_err(|_| AgentToolsError::LockPoisoned)?;
                // Re-check lease under the write lock.
                if let Some(outcome) = evict_if_advertisement_expired(&mut store, &plan, now) {
                    self.persist_locked(&mut store)?;
                    evicted.push(plan.peer.node_id);
                    outcomes.push(outcome);
                    continue;
                }
                let outcome =
                    apply_discovery_peer_stream_pages(&mut store, &plan.peer, fetched, now);
                if outcome.health == DiscoveryPeerHealth::Evicted {
                    store.outbound_discovery_peers.remove(&outcome.node_id);
                    evicted.push(outcome.node_id);
                }
                self.persist_locked(&mut store)?;
                outcome
            };
            retained_announcements =
                retained_announcements.saturating_add(outcome.retained_announcements);
            retained_withdrawals =
                retained_withdrawals.saturating_add(outcome.retained_withdrawals);
            outcomes.push(outcome);
        }

        Ok(DiscoveryPeerSyncReport {
            outcomes,
            retained_announcements,
            retained_withdrawals,
            evicted,
        })
    }

    /// Whether automatic peer gossip is currently enabled.
    #[must_use]
    pub fn peer_gossip_enabled(&self) -> bool {
        self.store
            .read()
            .map(|store| peer_gossip_is_enabled(&store))
            .unwrap_or(true)
    }

    /// Projects a verified announcement into the peer serving stream while enabled.
    ///
    /// Requires Administration. Verifies the announcement signature and active
    /// lease before appending to the peer-local stream.
    ///
    /// # Errors
    ///
    /// Returns authorization errors, rejection when service is disabled or the
    /// announcement fails verify/lease checks, or persistence errors.
    pub fn project_peer_serving_announcement(
        &self,
        ctx: &AuthContext,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<u64, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let sequence =
            project_peer_serving_announcement(&mut store, announcement, now).map_err(|reason| {
                AgentToolsError::DiscoveryPeerRejected {
                    message: format!("discovery peer project rejected: {reason}"),
                    reason,
                }
            })?;
        self.persist_locked(&mut store)?;
        Ok(sequence)
    }
}
