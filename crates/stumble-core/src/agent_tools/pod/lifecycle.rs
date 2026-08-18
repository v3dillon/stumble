use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Routes Pod creation through the sensitive-change policy.
    ///
    /// # Errors
    ///
    /// Returns an error when private creation or public proposal authorization,
    /// validation, signing, or persistence fails.
    pub fn request_create_pod(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<CreatePodOutcome, AgentToolsError> {
        if request.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::CreatePublicPod { request },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| CreatePodOutcome::PendingApproval(Box::new(proposal)));
        }
        self.create_pod(ctx, request).map(CreatePodOutcome::Created)
    }

    /// Atomically creates a Pod with its selected initial package, routing
    /// public exposure through a Pending Proposal.
    pub fn request_create_pod_lifecycle(
        &self,
        ctx: &AuthContext,
        request: CreatePodLifecycleRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<CreatePodOutcome, AgentToolsError> {
        if request.pod.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::CreatePublicPodLifecycle { request },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| CreatePodOutcome::PendingApproval(Box::new(proposal)));
        }
        self.create_pod_lifecycle_immediately(ctx, request, PodCreationMode::Canonical)
            .map(|created| CreatePodOutcome::Created(created.pod))
    }

    pub(crate) fn create_pod_lifecycle_immediately(
        &self,
        ctx: &AuthContext,
        request: CreatePodLifecycleRequest,
        mode: PodCreationMode,
    ) -> Result<CreatedPodPackage, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
        if !matches!(request.package, PodCreationPackage::Default) {
            authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PackageManagement)?;
        }
        let mut staged = store.clone();
        let created = stage_pod_lifecycle(&mut staged, ctx, request, ctx.harness_id, mode)?;
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(created)
    }

    /// Changes Pod visibility directly for restrictions and proposes expansions.
    pub fn request_set_pod_visibility(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        visibility: Visibility,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodVisibilityOutcome, AgentToolsError> {
        let current = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_pod_role_owner(&store, ctx, pod_id)?;
            let pod = store
                .pods
                .get(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            pod.visibility.clone()
        };
        if current == visibility {
            return Err(StoreError::Validation("Pod already has that visibility".into()).into());
        }
        if visibility_exposure(&visibility) > visibility_exposure(&current) {
            // Agent Harnesses need an independent approver (ADR-0033); the Home
            // Node Owner acting directly is that approver, so apply immediately.
            if ctx.harness_id.is_some() {
                return self
                    .create_pending_proposal_from_request(
                        ctx,
                        CreatePendingProposalRequest {
                            requested_change: SensitiveChange::ExpandPodVisibility {
                                pod_id,
                                visibility,
                            },
                            expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                        },
                        now,
                    )
                    .map(|proposal| PodVisibilityOutcome::PendingApproval(Box::new(proposal)));
            }
            let mut store = self
                .store
                .write()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_pod_role_owner(&store, ctx, pod_id)?;
            apply_expand_pod_visibility(&mut store, ctx, &pod_id, &visibility)?;
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::CreatePod,
                Some(pod_id),
            );
            self.persist_locked(&mut store)?;
            let pod = store
                .pods
                .get(&pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            return Ok(PodVisibilityOutcome::Updated(pod));
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_pod_role_owner(&store, ctx, pod_id)?;
        let was_public = current == Visibility::Public;
        let pod = store
            .pods
            .get_mut(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
        pod.visibility = visibility;
        let result = pod.clone();
        if let Some(rules) = store.pod_rules.get_mut(&pod_id) {
            rules.federate_sources = result.visibility == Visibility::Public;
        }
        if was_public && result.visibility != Visibility::Public {
            let node = store.node_for_tenant(ctx.tenant_id)?;
            issue_origin_pod_withdrawal(&mut store, &node, &result.slug, None, now)?;
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::WithdrawPublicPod,
                Some(pod_id),
            );
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreatePod,
            Some(pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(PodVisibilityOutcome::Updated(result))
    }

    /// Deletes a locally owned Pod. A harness that targets a public Pod
    /// receives a Pending Proposal; the Home Node Owner applies the change.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller is not the Owner, the Pod is the
    /// Inbox or a remote replica, authorization is denied, or persistence fails.
    pub fn request_delete_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DeletePodOutcome, AgentToolsError> {
        let is_public = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_pod_role_owner(&store, ctx, pod_id)?;
            let pod = store
                .pods
                .get(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            if is_private_inbox(pod) {
                return Err(
                    StoreError::Validation("the private Inbox cannot be deleted".into()).into(),
                );
            }
            let node = store.node_for_tenant(ctx.tenant_id)?;
            if pod
                .origin_node_id
                .is_some_and(|origin_node_id| origin_node_id != node.id)
            {
                return Err(StoreError::Validation(
                    "this Pod is a replica of a remote Origin; unsubscribe instead of deleting"
                        .into(),
                )
                .into());
            }
            pod.visibility == Visibility::Public
        };
        if is_public && ctx.harness_id.is_some() {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::DeletePod { pod_id },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| DeletePodOutcome::PendingApproval(Box::new(proposal)));
        }
        self.delete_pod_immediately(ctx, pod_id, now)
            .map(DeletePodOutcome::Deleted)
    }

    pub(crate) fn delete_pod_immediately(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DeletedPod, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_pod_role_owner(&store, ctx, pod_id)?;
        let mut staged = store.clone();
        let deleted = delete_owned_pod_locked(&mut staged, ctx, pod_id, now)?;
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(deleted)
    }

    pub fn create_pod(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
    ) -> Result<Pod, AgentToolsError> {
        if request.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public exposure requires a Pending Proposal".to_string(),
            )
            .into());
        }
        self.create_pod_lifecycle_immediately(
            ctx,
            CreatePodLifecycleRequest {
                pod: request,
                package: PodCreationPackage::Default,
            },
            PodCreationMode::SimpleCreate,
        )
        .map(|created| created.pod)
    }

    #[cfg(test)]
    pub(crate) fn create_pod_for_test(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
    ) -> Result<Pod, AgentToolsError> {
        self.create_pod_lifecycle_immediately(
            ctx,
            CreatePodLifecycleRequest {
                pod: request,
                package: PodCreationPackage::Default,
            },
            PodCreationMode::SimpleCreate,
        )
        .map(|created| created.pod)
    }

    /// Atomically creates a private Pod and its complete initial Pod Package.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization or validation fails, the slug is in
    /// use, signing fails, or persistence cannot commit the complete operation.
    pub fn create_private_pod_with_package(
        &self,
        ctx: &AuthContext,
        request: CreatePrivatePodWithPackageRequest,
    ) -> Result<CreatedPodPackage, AgentToolsError> {
        self.create_pod_lifecycle_immediately(
            ctx,
            CreatePodLifecycleRequest {
                pod: CreatePodRequest {
                    name: request.name,
                    slug: request.slug,
                    description: request.description,
                    visibility: Visibility::Private,
                },
                package: PodCreationPackage::Initial {
                    package: request.package,
                },
            },
            PodCreationMode::PrivatePackage,
        )
    }

    /// Creates Feed eligibility for a local Pod without granting Pod authority.
    pub fn subscribe_local_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Subscription, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod.id),
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Subscription requires an authenticated User".into())
        })?;
        if let Some(subscription) = store.subscriptions.values().find(|subscription| {
            subscription.user_id == user_id && subscription.local_pod_id == pod.id
        }) {
            return Ok(subscription.clone());
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let now = Utc::now();
        let subscription =
            Subscription::new_local(Uuid::now_v7().into(), user_id, &pod, &node, now);
        store
            .subscriptions
            .insert(subscription.id, subscription.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::JoinPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(subscription)
    }

    pub fn join_pod(&self, ctx: &AuthContext, pod_slug: &str) -> Result<(), AgentToolsError> {
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        self.subscribe_local_pod(ctx, pod.id).map(|_| ())
    }

    /// Removes Feed eligibility while leaving all Pod Roles unchanged.
    pub fn unsubscribe_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Subscription, AgentToolsError> {
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
            StoreError::Validation("unsubscribe requires an authenticated User".into())
        })?;
        let subscription_id = store
            .subscriptions
            .values()
            .find(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
            .map(|subscription| subscription.id)
            .ok_or_else(|| StoreError::NotFound("Subscription".into()))?;
        let subscription = store
            .subscriptions
            .remove(&subscription_id)
            .expect("Subscription was resolved above");
        self.persist_locked(&mut store)?;
        Ok(subscription)
    }

    /// Requests an Owner-authorized Pod Role grant through independent approval.
    pub fn request_grant_pod_role(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        user_id: UserId,
        role: PodRole,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal(
            ctx,
            SensitiveChange::GrantPodRole {
                pod_id,
                user_id,
                role,
            },
            now,
            now + Duration::hours(24),
        )
    }

    /// Requests an Owner-authorized Pod Role revocation through independent approval.
    pub fn request_revoke_pod_role(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        user_id: UserId,
        role: PodRole,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal(
            ctx,
            SensitiveChange::RevokePodRole {
                pod_id,
                user_id,
                role,
            },
            now,
            now + Duration::hours(24),
        )
    }

    /// Changes a Pod's curation policy; Autonomous Curation must use a Pending Proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied, the Pod is remote or missing,
    /// Autonomous Curation is requested directly, or persistence fails.
    pub fn set_pod_curation_policy(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        policy: CurationPolicy,
        now: chrono::DateTime<Utc>,
    ) -> Result<CurationPolicy, AgentToolsError> {
        if matches!(policy, CurationPolicy::Autonomous { .. }) {
            return Err(StoreError::Validation(
                "Autonomous Curation requires a Pending Proposal".into(),
            )
            .into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        store.pod_curation_policies.insert(pod_id, policy);
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::SetPodCurationPolicy,
            Some(pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(policy)
    }
}
