use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Creates an expiring proposal without applying its sensitive change.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller is not an authenticated harness, lacks
    /// authority for the affected resource, supplies an invalid expiry, or
    /// persistence fails.
    pub fn create_pending_proposal(
        &self,
        ctx: &AuthContext,
        requested_change: SensitiveChange,
        now: chrono::DateTime<Utc>,
        expires_at: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let proposer = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
            reason: "Pending Proposals require an authenticated Agent Harness".to_string(),
        })?;
        let proposer_harness =
            harness_for_context(&store, ctx)?.ok_or_else(|| AgentToolsError::Forbidden {
                reason: "Pending Proposals require an authenticated Agent Harness".to_string(),
            })?;
        let proposer_user_id = proposer_harness.user_id;
        let proposer_tenant_id = proposer_harness.tenant_id;
        if expires_at <= now || expires_at > now + Duration::days(7) {
            return Err(StoreError::Validation(
                "Pending Proposal expiry must be within seven days".to_string(),
            )
            .into());
        }
        let (affected_resources, expected_consequences, structured_diff) = match &requested_change {
            SensitiveChange::CreatePublicPod { request } => {
                authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
                if request.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "CreatePublicPod requires public visibility".to_string(),
                    )
                    .into());
                }
                if store
                    .pods
                    .values()
                    .any(|pod| pod.slug == request.slug && pod.tenant_id == ctx.tenant_id)
                {
                    return Err(StoreError::Duplicate(format!("pod {}", request.slug)).into());
                }
                let resource = ProposalResource::PodSlug(request.slug.clone());
                (
                    vec![resource.clone()],
                    vec!["A new Pod and its signed Package become immediately available through federation and Explore surfaces.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: serde_json::Value::Null,
                        after: json!(request),
                    }],
                )
            }
            SensitiveChange::CreatePublicPodLifecycle { request } => {
                authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
                if !matches!(request.package, PodCreationPackage::Default) {
                    authorize_harness_for_new_pod(
                        &store,
                        ctx,
                        HarnessCapability::PackageManagement,
                    )?;
                }
                if request.pod.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "public Pod lifecycle creation requires public visibility".into(),
                    )
                    .into());
                }
                if store
                    .pods
                    .values()
                    .any(|pod| pod.slug == request.pod.slug && pod.tenant_id == ctx.tenant_id)
                {
                    return Err(StoreError::Duplicate(format!("pod {}", request.pod.slug)).into());
                }
                validate_creation_package_locked(&store, ctx, &request.package)?;
                let resource = ProposalResource::PodSlug(request.pod.slug.clone());
                (
                    vec![resource.clone()],
                    vec!["A new Pod and its selected signed Package become available atomically through federation and Explore surfaces.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: serde_json::Value::Null,
                        after: json!(request),
                    }],
                )
            }
            SensitiveChange::PublishPod { pod_id } => {
                authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(*pod_id))?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if pod.visibility == Visibility::Public {
                    return Err(StoreError::Validation("Pod is already public".to_string()).into());
                }
                let resource = ProposalResource::Pod(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The Pod and its signed public events become available through federation and Explore surfaces.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"visibility": pod.visibility}),
                        after: json!({"visibility": Visibility::Public}),
                    }],
                )
            }
            SensitiveChange::ExpandPodVisibility { pod_id, visibility } => {
                authorize_pod_role_owner(&store, ctx, *pod_id)?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if visibility_exposure(visibility) <= visibility_exposure(&pod.visibility) {
                    return Err(StoreError::Validation(
                        "Pending Proposals only apply to visibility expansion".into(),
                    )
                    .into());
                }
                let resource = ProposalResource::Pod(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The Pod becomes visible to a broader audience.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"visibility": pod.visibility}),
                        after: json!({"visibility": visibility}),
                    }],
                )
            }
            SensitiveChange::ExpandHarnessGrant {
                harness_id,
                capabilities,
                pod_ids,
            } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                let target = store
                    .agent_harnesses
                    .get(harness_id)
                    .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {harness_id}")))?;
                store.assert_tenant(target.tenant_id, ctx.tenant_id)?;
                for pod_id in pod_ids.iter().flatten() {
                    let pod = store
                        .pods
                        .get(pod_id)
                        .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                    store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                }
                let normalized_capabilities = normalize_capabilities(capabilities.clone());
                if target
                    .grant
                    .capabilities
                    .iter()
                    .any(|capability| !normalized_capabilities.contains(capability))
                    || !grant_scope_expands(&target.grant.pod_ids, pod_ids)
                {
                    return Err(StoreError::Validation(
                        "sensitive grant change must only expand authority".to_string(),
                    )
                    .into());
                }
                if target.kind == AgentHarnessKind::Unattended
                    && normalized_capabilities.iter().any(|capability| {
                        matches!(
                            capability,
                            HarnessCapability::Administration | HarnessCapability::Approval
                        )
                    })
                {
                    return Err(AgentToolsError::Forbidden {
                        reason: "unattended harnesses cannot receive administration or approval"
                            .to_string(),
                    });
                }
                let resource = ProposalResource::AgentHarness(*harness_id);
                (
                    vec![resource.clone()],
                    vec![
                        "The Harness Grant gains additional authority for future requests."
                            .to_string(),
                    ],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!(target.grant),
                        after: json!(HarnessGrant {
                            capabilities: normalized_capabilities,
                            pod_ids: pod_ids.clone().map(normalize_pod_ids),
                        }),
                    }],
                )
            }
            SensitiveChange::AddTrustedPeer {
                node_id,
                display_name,
                base_url,
                public_key,
            } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                if display_name.trim().is_empty()
                    || public_key.trim().is_empty()
                    || Url::parse(base_url).is_err()
                {
                    return Err(StoreError::Validation(
                        "trusted peer name, URL, and public key must be valid".to_string(),
                    )
                    .into());
                }
                if store.trusted_peers.values().any(|peer| {
                    peer.tenant_id == ctx.tenant_id
                        && (peer.base_url == *base_url
                            || (!node_id.is_nil() && peer.node_id == *node_id))
                }) {
                    return Err(StoreError::Duplicate(format!("trusted peer {base_url}")).into());
                }
                let resource = ProposalResource::TrustedPeerUrl(base_url.clone());
                (
                    vec![resource.clone()],
                    vec!["The Home Node will trust signed public data from this peer.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: serde_json::Value::Null,
                        after: json!({
                            "node_id": node_id,
                            "display_name": display_name,
                            "base_url": base_url,
                            "public_key": public_key,
                            "trust_level": TrustLevel::ReadOnly,
                            "enabled": true,
                        }),
                    }],
                )
            }
            SensitiveChange::RemoveTrustedPeer { peer_id } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                let peer = store
                    .trusted_peers
                    .get(peer_id)
                    .ok_or_else(|| StoreError::NotFound(format!("trusted peer {peer_id}")))?;
                store.assert_tenant(peer.tenant_id, ctx.tenant_id)?;
                if !peer.enabled {
                    return Err(
                        StoreError::Validation("trusted peer is already disabled".into()).into(),
                    );
                }
                let resource = ProposalResource::TrustedPeerUrl(peer.base_url.clone());
                let mut disabled = peer.clone();
                disabled.enabled = false;
                (
                    vec![resource.clone()],
                    vec!["The peer can no longer exchange signed public discovery data with this Home Node.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!(peer),
                        after: json!(disabled),
                    }],
                )
            }
            SensitiveChange::ChangeTrustPolicy { change } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                let current = store
                    .trust_policies
                    .get(&(proposer_user_id, proposer_tenant_id))
                    .cloned()
                    .unwrap_or_else(|| TrustPolicy::new(proposer_user_id, proposer_tenant_id));
                let mut prospective = current.clone();
                apply_trust_policy_change(&mut prospective, change)?;
                let resource = ProposalResource::TrustPolicy(proposer_user_id);
                (
                    vec![resource.clone()],
                    vec!["The Home Node's local public Pod discovery rules change.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!(current),
                        after: json!(prospective),
                    }],
                )
            }
            SensitiveChange::RevisePublicPodPackage {
                pod_id,
                base_version,
                patch,
            } => {
                authorize_harness(
                    &store,
                    ctx,
                    HarnessCapability::PackageManagement,
                    Some(*pod_id),
                )?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if pod.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "this proposal type requires a public Pod".to_string(),
                    )
                    .into());
                }
                ensure_direct_package_revision_allowed_for_origin(&store, ctx, pod)?;
                let existing = store
                    .pod_skill_packs
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
                if PackageVersion::new(existing.version)
                    .map_err(|error| StoreError::Validation(error.to_string()))?
                    != *base_version
                {
                    return Err(StoreError::Validation(
                        "public Package Revision base version is stale".to_string(),
                    )
                    .into());
                }
                let prospective = patch_skill_pack(existing, patch.clone());
                let validation = validate_skill_pack(&prospective);
                if !validation.valid {
                    return Err(StoreError::Validation(validation.errors.join(", ")).into());
                }
                let pod_resource = ProposalResource::Pod(*pod_id);
                let package_resource = ProposalResource::PodPackage(*pod_id);
                (
                    vec![pod_resource, package_resource.clone()],
                    vec![
                        "The signed public Pod Package changes for current and future subscribers."
                            .to_string(),
                    ],
                    vec![ProposalResourceDiff {
                        resource: package_resource,
                        before: json!(existing),
                        after: json!(prospective),
                    }],
                )
            }
            SensitiveChange::RemovePublicSubmissionFromPod {
                pod_id,
                submission_id,
            } => {
                authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(*pod_id))?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if pod.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "this proposal type requires a public Pod".to_string(),
                    )
                    .into());
                }
                if !store.submission_pods.iter().any(|placement| {
                    placement.pod_id == *pod_id && placement.submission_id == *submission_id
                }) {
                    return Err(StoreError::NotFound(format!(
                        "submission {submission_id} in pod {}",
                        pod.slug
                    ))
                    .into());
                }
                let resource = ProposalResource::SubmissionPlacement {
                    pod_id: *pod_id,
                    submission_id: *submission_id,
                };
                (
                    vec![resource.clone()],
                    vec!["The public Pod Placement is withdrawn from future federation and discovery.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"accepted": true}),
                        after: json!({"accepted": false}),
                    }],
                )
            }
            SensitiveChange::EnableAutonomousCuration {
                pod_id,
                confidence_threshold,
            } => {
                authorize_local_pod_curation(&store, ctx, *pod_id)?;
                let current = store
                    .pod_curation_policies
                    .get(pod_id)
                    .copied()
                    .unwrap_or_default();
                if matches!(current, CurationPolicy::Autonomous { .. }) {
                    return Err(StoreError::Validation(
                        "Pod already uses Autonomous Curation".into(),
                    )
                    .into());
                }
                let resource = ProposalResource::PodCurationPolicy(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The Pod may accept qualifying Candidate Placements without manual or trusted-source review.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"curation_policy": current}),
                        after: json!({
                            "curation_policy": CurationPolicy::Autonomous {
                                confidence_threshold: *confidence_threshold,
                            }
                        }),
                    }],
                )
            }
            SensitiveChange::GrantPodRole {
                pod_id,
                user_id,
                role,
            } => {
                authorize_pod_role_owner(&store, ctx, *pod_id)?;
                if !store.users.contains_key(user_id) {
                    return Err(StoreError::NotFound(format!("User {user_id}")).into());
                }
                if store.pod_roles.iter().any(|assignment| {
                    assignment.pod_id == *pod_id
                        && assignment.user_id == *user_id
                        && assignment.role == *role
                }) {
                    return Err(
                        StoreError::Duplicate(format!("Pod Role for User {user_id}")).into(),
                    );
                }
                if *role != PodRole::Owner
                    && store.pod_roles.iter().any(|assignment| {
                        assignment.pod_id == *pod_id
                            && assignment.user_id == *user_id
                            && assignment.role == PodRole::Owner
                    })
                    && store
                        .pod_roles
                        .iter()
                        .filter(|assignment| {
                            assignment.pod_id == *pod_id && assignment.role == PodRole::Owner
                        })
                        .count()
                        == 1
                {
                    return Err(
                        StoreError::Validation("cannot replace the last Pod Owner".into()).into(),
                    );
                }
                let before = pod_roles_value(&store, *pod_id);
                let mut prospective = store.clone();
                prospective.pod_roles.retain(|assignment| {
                    assignment.pod_id != *pod_id || assignment.user_id != *user_id
                });
                prospective.pod_roles.push(PodRoleAssignment {
                    user_id: *user_id,
                    pod_id: *pod_id,
                    role: role.clone(),
                    created_at: now,
                });
                let resource = ProposalResource::PodRoles(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The User gains explicit authority over this Pod.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before,
                        after: pod_roles_value(&prospective, *pod_id),
                    }],
                )
            }
            SensitiveChange::RevokePodRole {
                pod_id,
                user_id,
                role,
            } => {
                authorize_pod_role_owner(&store, ctx, *pod_id)?;
                let assignment = store
                    .pod_roles
                    .iter()
                    .find(|assignment| {
                        assignment.pod_id == *pod_id
                            && assignment.user_id == *user_id
                            && assignment.role == *role
                    })
                    .cloned()
                    .ok_or_else(|| StoreError::NotFound(format!("Pod Role for User {user_id}")))?;
                if assignment.role == PodRole::Owner
                    && store
                        .pod_roles
                        .iter()
                        .filter(|candidate| {
                            candidate.pod_id == *pod_id && candidate.role == PodRole::Owner
                        })
                        .count()
                        == 1
                {
                    return Err(
                        StoreError::Validation("cannot revoke the last Pod Owner".into()).into(),
                    );
                }
                let before = pod_roles_value(&store, *pod_id);
                let mut prospective = store.clone();
                prospective
                    .pod_roles
                    .retain(|candidate| candidate != &assignment);
                let resource = ProposalResource::PodRoles(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The User loses explicit authority over this Pod.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before,
                        after: pod_roles_value(&prospective, *pod_id),
                    }],
                )
            }
        };
        let proposal = PendingProposal {
            id: PendingProposalId::from(Uuid::now_v7()),
            requested_change,
            affected_resources,
            expected_consequences,
            structured_diff,
            proposer,
            user_id: proposer_user_id,
            tenant_id: proposer_tenant_id,
            created_at: now,
            expires_at,
            status: ProposalStatus::Pending,
            decided_by: None,
            decided_at: None,
            rejection_reason: None,
        };
        store
            .pending_proposals
            .insert(proposal.id, proposal.clone());
        self.persist_locked(&mut store)?;
        Ok(proposal)
    }

    /// Creates a proposal from the transport-neutral relative-expiry request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::create_pending_proposal`] and rejects
    /// durations that cannot be represented safely.
    pub fn create_pending_proposal_from_request(
        &self,
        ctx: &AuthContext,
        request: CreatePendingProposalRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let seconds = i64::try_from(request.expires_in_seconds).map_err(|_| {
            StoreError::Validation("Pending Proposal expiry is too large".to_string())
        })?;
        self.create_pending_proposal(
            ctx,
            request.requested_change,
            now,
            now + Duration::seconds(seconds),
        )
    }

    /// Returns one proposal and records expiry when it is first observed late.
    ///
    /// # Errors
    ///
    /// Returns an error when the proposal is missing, the caller is neither a
    /// local owner nor an authorized participant, or persistence fails.
    pub fn pending_proposal(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_proposal_reader(&store, ctx, proposal_id)?;
        expire_proposal(&mut store, proposal_id, now)?;
        let proposal = store
            .pending_proposals
            .get(&proposal_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        self.persist_locked(&mut store)?;
        Ok(proposal)
    }

    /// Lists Pending Proposals visible to the acting User or Harness Grant.
    pub fn list_pending_proposals(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<PendingProposal>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        // Validate a supplied Harness identity even when there are no visible proposals.
        let _ = harness_for_context(&store, ctx)?;
        let proposal_ids = store.pending_proposals.keys().copied().collect::<Vec<_>>();
        for proposal_id in proposal_ids {
            expire_proposal(&mut store, proposal_id, now)?;
        }
        let mut proposals = store
            .pending_proposals
            .values()
            .filter(|proposal| authorize_proposal_reader(&store, ctx, proposal.id).is_ok())
            .cloned()
            .collect::<Vec<_>>();
        proposals.sort_by_key(|proposal| proposal.created_at);
        self.persist_locked(&mut store)?;
        Ok(proposals)
    }

    /// Returns the proposal decisions currently allowed for this actor.
    pub fn pending_proposal_allowed_actions(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
    ) -> Result<Vec<ProposalAllowedAction>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let proposal = store
            .pending_proposals
            .get(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        authorize_proposal_reader(&store, ctx, proposal_id)?;
        if proposal.status == ProposalStatus::Pending
            && authorize_independent_approver(&store, ctx, proposal_id).is_ok()
        {
            Ok(vec![
                ProposalAllowedAction::Approve,
                ProposalAllowedAction::Reject,
            ])
        } else {
            Ok(Vec::new())
        }
    }

    /// Independently approves and atomically applies a live proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when approval authority or independence is missing,
    /// the proposal is expired or terminal, the change is no longer valid, or
    /// persistence fails.
    pub fn approve_pending_proposal(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let approver = authorize_independent_approver(&store, ctx, proposal_id)?;
        expire_proposal(&mut store, proposal_id, now)?;
        let proposal_status = store
            .pending_proposals
            .get(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?
            .status;
        if proposal_status != ProposalStatus::Pending {
            self.persist_locked(&mut store)?;
            return Err(StoreError::Validation("Pending Proposal is terminal".to_string()).into());
        }
        let proposal_snapshot = store
            .pending_proposals
            .get(&proposal_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        validate_structured_diff(&store, &proposal_snapshot)?;
        let requested_change = proposal_snapshot.requested_change;
        let proposer = proposal_snapshot.proposer;
        let before_approval = store.clone();
        if let Err(error) = apply_sensitive_change(&mut store, ctx, proposer, &requested_change) {
            *store = before_approval;
            return Err(error);
        }
        let proposal = store
            .pending_proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        proposal.status = ProposalStatus::Accepted;
        proposal.decided_by = Some(approver);
        proposal.decided_at = Some(now);
        let proposal = proposal.clone();
        if let Err(error) = self.persist_locked(&mut store) {
            return Err(error);
        }
        Ok(proposal)
    }

    /// Independently rejects a live proposal without applying its change.
    ///
    /// # Errors
    ///
    /// Returns an error when approval authority or independence is missing,
    /// the reason is empty, the proposal is expired or terminal, or
    /// persistence fails.
    pub fn reject_pending_proposal(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
        now: chrono::DateTime<Utc>,
        reason: String,
    ) -> Result<PendingProposal, AgentToolsError> {
        if reason.trim().is_empty() {
            return Err(
                StoreError::Validation("rejection reason must not be empty".to_string()).into(),
            );
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let approver = authorize_independent_approver(&store, ctx, proposal_id)?;
        expire_proposal(&mut store, proposal_id, now)?;
        let proposal_status = store
            .pending_proposals
            .get(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?
            .status;
        if proposal_status != ProposalStatus::Pending {
            self.persist_locked(&mut store)?;
            return Err(StoreError::Validation("Pending Proposal is terminal".to_string()).into());
        }
        let proposal = store
            .pending_proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        proposal.status = ProposalStatus::Rejected;
        proposal.decided_by = Some(approver);
        proposal.decided_at = Some(now);
        proposal.rejection_reason = Some(reason);
        let proposal = proposal.clone();
        self.persist_locked(&mut store)?;
        Ok(proposal)
    }
}
