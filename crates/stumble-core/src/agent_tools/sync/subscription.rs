use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Pod manifest for the federation surface. A non-public pod is reported as
    /// `NotFound` — byte-identical to a missing pod — so private pods cannot be
    /// probed for existence through this endpoint.
    pub fn federation_pod_manifest(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodManifest, AgentToolsError> {
        let node_id = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            store.node_for_tenant(ctx.tenant_id)?.id
        };
        let manifest = self.pod_manifest(ctx, pod_slug)?;
        if manifest.pod.visibility != Visibility::Public {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        if manifest
            .pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node_id)
        {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        Ok(manifest)
    }

    /// Pod event log for the federation surface. A non-public pod is reported as
    /// `NotFound` so private pods never expose their events.
    pub fn federation_pod_events(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<Vec<EventLog>, AgentToolsError> {
        let node_id = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            store.node_for_tenant(ctx.tenant_id)?.id
        };
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        if pod.visibility != Visibility::Public {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node_id)
        {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        self.export_pod_events(ctx, pod_slug)
    }

    /// Exports one public Pod's signed artifacts after an optional event cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not locally authoritative and public,
    /// the cursor is unknown, or the Home Node store lock is poisoned.
    pub fn federation_pod_snapshot(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        after_event_hash: Option<&str>,
    ) -> Result<FederationPodSnapshot, AgentToolsError> {
        let node = self.node_info(ctx)?;
        let manifest = self.federation_pod_manifest(ctx, pod_slug)?;
        let all_events = self.federation_pod_events(ctx, pod_slug)?;
        let events = match after_event_hash {
            Some(cursor) => {
                let index = all_events
                    .iter()
                    .position(|event| event.content_hash == cursor)
                    .ok_or_else(|| {
                        StoreError::Validation("synchronization cursor is unknown".to_string())
                    })?;
                all_events.into_iter().skip(index + 1).collect()
            }
            None => all_events,
        };
        Ok(FederationPodSnapshot {
            node,
            manifest,
            events,
        })
    }

    /// Subscribes to a directly addressed public Pod and projects verified artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied, the public URL is invalid,
    /// signed artifacts fail verification, or persistence fails.
    pub fn subscribe_public_pod(
        &self,
        ctx: &AuthContext,
        request: SubscribePublicPodRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<SynchronizationResult, AgentToolsError> {
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Subscription requires an authenticated User".to_string())
        })?;
        let public_pod_url =
            validate_public_pod_url(&request.public_pod_url, &request.snapshot.manifest.pod.slug)?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::SubscriptionManagement, None)?;
        if store.subscriptions.values().any(|subscription| {
            subscription.user_id == user_id
                && subscription.tenant_id == ctx.tenant_id
                && subscription.public_pod_url == public_pod_url
        }) {
            return Err(StoreError::Duplicate(format!("Subscription to {public_pod_url}")).into());
        }
        validate_federation_snapshot(&store, ctx.tenant_id, None, &request.snapshot)?;
        let mut projected = store.clone();
        let imported_events =
            project_snapshot_events(&mut projected, ctx, &request.snapshot.events)?;
        let local_pod = projected
            .pods
            .values()
            .find(|pod| {
                pod.tenant_id == ctx.tenant_id
                    && pod.slug == request.snapshot.manifest.pod.slug
                    && pod.origin_node_id == Some(request.snapshot.node.node_id)
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound("synchronized public Pod".to_string()))?;
        let subscription = Subscription {
            id: Uuid::now_v7().into(),
            user_id,
            tenant_id: ctx.tenant_id,
            public_pod_url,
            origin_node_id: request.snapshot.node.node_id,
            origin_public_key: request.snapshot.node.public_key,
            pod_slug: request.snapshot.manifest.pod.slug,
            local_pod_id: local_pod.id,
            is_priority: false,
            last_event_hash: request.snapshot.manifest.latest_known_event_hash,
            created_at: now,
            synchronized_at: now,
            last_sync_failure: None,
        };
        projected
            .subscriptions
            .insert(subscription.id, subscription.clone());
        record_harness_write_at(
            &mut projected,
            ctx,
            HarnessWriteOperation::SubscribePublicPod,
            Some(local_pod.id),
            now,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        Ok(SynchronizationResult {
            subscription,
            imported_events,
        })
    }

    /// Applies the next contiguous signed event segment for a Subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied, Origin identity changes,
    /// the event chain is discontinuous or invalid, or persistence fails.
    pub fn synchronize_subscription(
        &self,
        ctx: &AuthContext,
        subscription_id: SubscriptionId,
        mut snapshot: FederationPodSnapshot,
    ) -> Result<SynchronizationResult, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let existing = store
            .subscriptions
            .get(&subscription_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(existing.local_pod_id),
        )?;
        if ctx.user_id != Some(existing.user_id)
            || existing.tenant_id != ctx.tenant_id
            || existing.origin_node_id != snapshot.node.node_id
            || existing.origin_public_key != snapshot.node.public_key
            || existing.pod_slug != snapshot.manifest.pod.slug
        {
            return Err(StoreError::Validation(
                "synchronization artifacts do not match the Subscription".to_string(),
            )
            .into());
        }
        discard_replayed_events(&store, existing.last_event_hash.as_deref(), &mut snapshot)?;
        validate_federation_snapshot(
            &store,
            ctx.tenant_id,
            existing.last_event_hash.as_deref(),
            &snapshot,
        )?;
        let mut projected = store.clone();
        let imported_events = project_snapshot_events(&mut projected, ctx, &snapshot.events)?;
        let synchronized_at = Utc::now();
        let subscription = projected
            .subscriptions
            .get_mut(&subscription_id)
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        subscription.last_event_hash = snapshot.manifest.latest_known_event_hash;
        subscription.synchronized_at = synchronized_at;
        subscription.last_sync_failure = None;
        let subscription = subscription.clone();
        record_harness_write_at(
            &mut projected,
            ctx,
            HarnessWriteOperation::SynchronizeSubscription,
            Some(subscription.local_pod_id),
            synchronized_at,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        Ok(SynchronizationResult {
            subscription,
            imported_events,
        })
    }

    /// Reads one local Subscription within the authenticated User boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the Subscription is missing, belongs to another
    /// User or tenant, or the store lock is poisoned.
    pub fn subscription(
        &self,
        ctx: &AuthContext,
        subscription_id: SubscriptionId,
    ) -> Result<Subscription, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let subscription = store
            .subscriptions
            .get(&subscription_id)
            .filter(|subscription| {
                Some(subscription.user_id) == ctx.user_id && subscription.tenant_id == ctx.tenant_id
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(subscription.local_pod_id),
        )?;
        Ok(subscription)
    }

    /// Resolves the authenticated User's Subscription for one local Pod projection.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not subscribed by this User or authorization is denied.
    pub fn subscription_for_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Subscription, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let subscription = store
            .subscriptions
            .values()
            .find(|subscription| {
                subscription.local_pod_id == pod_id
                    && Some(subscription.user_id) == ctx.user_id
                    && subscription.tenant_id == ctx.tenant_id
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription for Pod {pod_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod_id),
        )?;
        Ok(subscription)
    }

    /// Records an operator-visible failure without changing synchronized Pod state.
    ///
    /// # Errors
    ///
    /// Returns an error when the Subscription is inaccessible or persistence fails.
    pub fn record_subscription_sync_failure(
        &self,
        ctx: &AuthContext,
        subscription_id: SubscriptionId,
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<Subscription, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let existing = store
            .subscriptions
            .get(&subscription_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(existing.local_pod_id),
        )?;
        if ctx.user_id != Some(existing.user_id) || ctx.tenant_id != existing.tenant_id {
            return Err(StoreError::NotFound(format!("Subscription {subscription_id}")).into());
        }
        let subscription = store
            .subscriptions
            .get_mut(&subscription_id)
            .expect("checked above");
        subscription.last_sync_failure = Some(SynchronizationFailure {
            code: code.into(),
            message: message.into(),
            retryable,
            occurred_at: now,
        });
        let subscription = subscription.clone();
        self.persist_locked(&mut store)?;
        Ok(subscription)
    }

    /// Configures bounded Priority Subscription representation in future Feed Batches.
    ///
    /// # Errors
    ///
    /// Returns an error when Subscription management is denied, the User is not
    /// subscribed to the Pod, or persistence fails.
    pub fn set_priority_subscription(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        is_priority: bool,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod_id),
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Priority Subscription requires an authenticated User".into())
        })?;
        let subscription = store
            .subscriptions
            .values_mut()
            .find(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
            .ok_or_else(|| StoreError::NotFound("Subscription".into()))?;
        subscription.is_priority = is_priority;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SetPrioritySubscription,
            Some(pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn export_pod_events(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<Vec<EventLog>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        Ok(store.public_events_for_pod(&pod.slug))
    }

    pub fn import_pod_events(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        events: Vec<EventLog>,
    ) -> Result<usize, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .cloned()
            .ok_or(StoreError::UntrustedPeer)?;
        if !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let mut imported = 0;
        for mut event in events {
            if store.event_log.iter().any(|existing| {
                existing.event_id == event.event_id || existing.content_hash == event.content_hash
            }) {
                continue;
            }
            if !verify_event(&event, &peer.public_key)? {
                return Err(StoreError::InvalidSignature.into());
            }
            event.imported_from_peer_id = Some(peer_id);
            event.verified = true;
            event.tenant_id = ctx.tenant_id;
            project_imported_public_event(&mut store, ctx, &event)?;
            store.event_log.push(event);
            imported += 1;
        }
        if imported > 0 {
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::ImportPodEvents,
                None,
            );
            self.persist_locked(&mut store)?;
        }
        Ok(imported)
    }
}
