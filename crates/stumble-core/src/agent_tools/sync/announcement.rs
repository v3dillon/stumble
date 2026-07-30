use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    pub fn pod_manifest(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodManifest, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let public_source_summary = store
            .crawler_sources
            .values()
            .filter(|source| source.pod_id == pod.id && source.enabled)
            .map(|source| source.url.clone())
            .collect();
        Ok(PodManifest {
            pod: pod.clone(),
            latest_known_event_hash: store.latest_federated_event_hash(&pod.slug),
            skill_pack_version: pack.version,
            public_source_summary,
        })
    }

    /// Produces a compact signed advertisement for a public Origin Pod.
    ///
    /// The signed payload includes a renewable 30-day Announcement Lease
    /// (`expires_at = announced_at + 30 days`) reflecting current public metadata.
    /// The Origin retains the announcement so later public-state changes can
    /// refresh the lease against the same direct address.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not public or authoritative at this
    /// node, the direct address is invalid, signing fails, or state is unavailable.
    /// Re-signs and retains the current announcement for every local public
    /// Origin Pod that already has one, renewing Announcement Leases and
    /// capturing the latest federated event pointer.
    ///
    /// Detection is event-driven — announcements bind `latest_event_hash`, so
    /// re-signing simply asserts current state; callers push the results to
    /// Bootstrap endpoints, whose admission dedupes unchanged announcements.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization, signing, or persistence fails for
    /// any Pod, or when the store lock is poisoned.
    pub fn refresh_origin_pod_announcements(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<PodAnnouncement>, AgentToolsError> {
        let targets: Vec<(String, String)> = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            let node = store.node_for_tenant(ctx.tenant_id)?;
            store
                .pods
                .values()
                .filter(|pod| {
                    pod.tenant_id == ctx.tenant_id
                        && pod.visibility == Visibility::Public
                        && pod.origin_node_id.is_none_or(|origin| origin == node.id)
                })
                .filter_map(|pod| {
                    store
                        .known_pod_announcements
                        .get(&(node.id, pod.slug.clone()))
                        .map(|known| {
                            (pod.slug.clone(), known.announcement.public_pod_url.clone())
                        })
                })
                .collect()
        };
        let mut refreshed = Vec::with_capacity(targets.len());
        for (slug, public_pod_url) in targets {
            refreshed.push(self.pod_announcement_at(ctx, &slug, &public_pod_url, now)?);
        }
        Ok(refreshed)
    }

    pub fn pod_announcement(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        public_pod_url: &str,
    ) -> Result<PodAnnouncement, AgentToolsError> {
        self.pod_announcement_at(ctx, pod_slug, public_pod_url, Utc::now())
    }

    /// Produces a Pod Announcement at an explicit issuance time (testable clocks).
    ///
    /// Also used to renew an active lease: each call issues a fresh signature
    /// with a new `announced_at` / `expires_at` reflecting current public state.
    ///
    /// # Errors
    ///
    /// Same as [`Self::pod_announcement`].
    pub fn pod_announcement_at(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        public_pod_url: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodAnnouncement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        if pod.visibility != Visibility::Public {
            return Err(StoreError::NotFound(format!("public Pod {pod_slug}")).into());
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node.id)
        {
            return Err(StoreError::Validation(
                "only an Origin Node can announce its public Pod".into(),
            )
            .into());
        }
        let announcement =
            issue_and_retain_origin_pod_announcement(&mut store, &node, &pod, public_pod_url, now)?;
        self.persist_locked(&mut store)?;
        Ok(announcement)
    }

    /// Produces an Origin-signed Pod Withdrawal for a public Pod.
    ///
    /// When `make_private` is true, the local Pod is also restricted to private
    /// visibility. Existing Subscriptions and synchronized content are preserved.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not public or authoritative, ownership is
    /// denied, signing fails, or state is unavailable.
    pub fn withdraw_public_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        public_pod_url: Option<&str>,
        make_private: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodWithdrawal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?.clone();
        authorize_pod_role_owner(&store, ctx, pod.id)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node.id)
        {
            return Err(StoreError::Validation(
                "only an Origin Node can withdraw its public Pod".into(),
            )
            .into());
        }
        if pod.visibility != Visibility::Public && !make_private {
            return Err(StoreError::NotFound(format!("public Pod {pod_slug}")).into());
        }
        let withdrawal = issue_origin_pod_withdrawal(
            &mut store,
            &node,
            &pod.slug,
            public_pod_url.map(str::to_owned),
            now,
        )?;
        if make_private && pod.visibility != Visibility::Private {
            let pod_id = pod.id;
            let mut_pod = store
                .pods
                .get_mut(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            mut_pod.visibility = Visibility::Private;
            if let Some(rules) = store.pod_rules.get_mut(&pod_id) {
                rules.federate_sources = false;
            }
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::WithdrawPublicPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(withdrawal)
    }

    /// Verifies and retains an Origin-signed announcement delivered by a trusted peer.
    ///
    /// The immediate peer remains delivery provenance only and cannot replace the
    /// announcement's signer or alter its authoritative fields.
    ///
    /// # Errors
    ///
    /// Returns an error for an untrusted peer, invalid signature, stale package
    /// version, malformed direct address, denied administration, or persistence failure.
    pub fn receive_pod_announcement(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        announcement: PodAnnouncement,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        self.receive_pod_announcement_at(ctx, peer_id, announcement, Utc::now())
    }

    /// Verifies and retains a peer-delivered announcement at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::receive_pod_announcement`].
    pub fn receive_pod_announcement_at(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .ok_or(StoreError::UntrustedPeer)?;
        if peer.tenant_id != ctx.tenant_id || !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let known = retain_verified_pod_announcement(
            &mut store,
            announcement,
            DeliveryProvenance::peer(peer_id),
            now,
        )?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ReceivePodAnnouncement,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Verifies and retains an Origin-signed Pod Withdrawal from a trusted peer.
    ///
    /// # Errors
    ///
    /// Returns an error for an untrusted peer, invalid signature, stale withdrawal,
    /// denied administration, or persistence failure.
    pub fn receive_pod_withdrawal(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        withdrawal: PodWithdrawal,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        self.receive_pod_withdrawal_at(ctx, peer_id, withdrawal, Utc::now())
    }

    /// Retains a peer-delivered Pod Withdrawal at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::receive_pod_withdrawal`].
    pub fn receive_pod_withdrawal_at(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        withdrawal: PodWithdrawal,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .ok_or(StoreError::UntrustedPeer)?;
        if peer.tenant_id != ctx.tenant_id || !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let known = retain_verified_pod_withdrawal(&mut store, withdrawal, Some(peer_id), now)?;
        // Keep Bootstrap stream closed under co-located Index/peer withdraw retain.
        project_bootstrap_withdrawal(&mut store, &known.withdrawal, now);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ReceivePodWithdrawal,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(known)
    }

}
