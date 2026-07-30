use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Registers a harness and returns its bearer token exactly once.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks administration authority, a Pod
    /// scope is invalid, no User exists, or persistence fails.
    pub fn register_agent_harness(
        &self,
        ctx: &AuthContext,
        request: RegisterAgentHarnessRequest,
    ) -> Result<RegisterAgentHarnessResponse, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if ctx.harness_id.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "only the Home Node Owner may register an Agent Harness directly"
                    .to_string(),
            });
        }
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        if request.label.trim().is_empty() {
            return Err(StoreError::Validation(
                "Agent Harness label must not be empty".to_string(),
            )
            .into());
        }
        // When the caller has no User (node-level owner context), bind the
        // harness to the same stable owner User as `local_owner_auth_context`.
        // HashMap iteration order must not pick a different seed User, or Trust
        // Policy and Personal Discovery diverge for owner vs harness paths.
        let user_id = ctx
            .user_id
            .or_else(|| local_owner_user_id(&store))
            .ok_or_else(|| {
                StoreError::Validation("an Agent Harness must belong to a User".to_string())
            })?;
        for pod_id in request.pod_ids.iter().flatten() {
            let pod = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        }
        let capabilities = normalize_capabilities(request.capabilities);
        if request.kind == AgentHarnessKind::Unattended
            && capabilities.iter().any(|capability| {
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
        if ctx.harness_id.is_none()
            && capabilities.len() > 1
            && capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    HarnessCapability::Administration | HarnessCapability::Approval
                )
            })
        {
            return Err(StoreError::Validation(
                "bootstrap administration and approval grants must be isolated".to_string(),
            )
            .into());
        }
        if let Some(caller) = harness_for_context(&store, ctx)? {
            if request.kind == AgentHarnessKind::Interactive
                || capabilities.contains(&HarnessCapability::Administration)
            {
                return Err(AgentToolsError::Forbidden {
                    reason: "a harness cannot delegate interactive or administration authority"
                        .to_string(),
                });
            }
            if capabilities
                .iter()
                .any(|capability| !caller.grant.capabilities.contains(capability))
            {
                return Err(AgentToolsError::Forbidden {
                    reason: "a harness cannot delegate capabilities it does not hold".to_string(),
                });
            }
            ensure_child_pod_scope(&caller.grant.pod_ids, &request.pod_ids)?;
        }
        let harness = AgentHarness {
            id: AgentHarnessId::from(Uuid::now_v7()),
            user_id,
            tenant_id: ctx.tenant_id,
            label: request.label,
            kind: request.kind,
            grant: HarnessGrant {
                capabilities,
                pod_ids: request.pod_ids.map(normalize_pod_ids),
            },
            created_at: Utc::now(),
            revoked_at: None,
        };
        let token = new_plaintext_api_token();
        let api_token = ApiToken {
            id: Uuid::now_v7(),
            user_id,
            tenant_id: ctx.tenant_id,
            token_hash: hash_api_token(&token),
            label: format!("Harness: {}", harness.label),
            created_at: Utc::now(),
            last_used_at: None,
            revoked_at: None,
            harness_id: Some(harness.id),
        };
        store.agent_harnesses.insert(harness.id, harness.clone());
        store.api_tokens.insert(api_token.id, api_token);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RegisterAgentHarness,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(RegisterAgentHarnessResponse {
            harness,
            token: HarnessToken::new(token),
        })
    }

    /// Revokes a harness and all of its bearer tokens immediately.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks administration authority, the
    /// harness does not exist, or persistence fails.
    pub fn revoke_agent_harness(
        &self,
        ctx: &AuthContext,
        harness_id: AgentHarnessId,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if ctx.harness_id.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "only the Home Node Owner may revoke an Agent Harness directly".to_string(),
            });
        }
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let now = Utc::now();
        let harness = store
            .agent_harnesses
            .get_mut(&harness_id)
            .ok_or_else(|| StoreError::NotFound(format!("agent harness {harness_id}")))?;
        harness.revoked_at = Some(now);
        for token in store
            .api_tokens
            .values_mut()
            .filter(|token| token.harness_id == Some(harness_id))
        {
            token.revoked_at = Some(now);
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RevokeAgentHarness,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(())
    }

    /// Lists Agent Harness metadata without exposing bearer credentials or hashes.
    pub fn list_agent_harnesses(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<AgentHarnessView>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let mut harnesses = store
            .agent_harnesses
            .values()
            .filter(|harness| harness.tenant_id == ctx.tenant_id)
            .map(|harness| agent_harness_view(&store, harness))
            .collect::<Result<Vec<_>, _>>()?;
        harnesses.sort_by_key(|view| view.harness.created_at);
        Ok(harnesses)
    }

    /// Returns one Agent Harness's safe metadata view.
    pub fn agent_harness(
        &self,
        ctx: &AuthContext,
        harness_id: AgentHarnessId,
    ) -> Result<AgentHarnessView, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let harness = store
            .agent_harnesses
            .get(&harness_id)
            .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {harness_id}")))?;
        store.assert_tenant(harness.tenant_id, ctx.tenant_id)?;
        agent_harness_view(&store, harness)
    }

    /// Requests an authority expansion through the Pending Proposal policy.
    pub fn request_harness_grant_expansion(
        &self,
        ctx: &AuthContext,
        harness_id: AgentHarnessId,
        capabilities: Vec<HarnessCapability>,
        pod_ids: Option<Vec<PodId>>,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal(
            ctx,
            SensitiveChange::ExpandHarnessGrant {
                harness_id,
                capabilities,
                pod_ids,
            },
            now,
            now + Duration::hours(24),
        )
    }

    /// Returns local-only harness write attribution records.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks administration authority or the
    /// Home Node store lock is poisoned.
    pub fn list_harness_write_audit(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<HarnessWriteAudit>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(store.harness_write_audit.clone())
    }

    /// Verifies a non-Pod-specific capability for the current harness context.
    /// Local non-harness contexts remain unrestricted for compatibility.
    ///
    /// # Errors
    ///
    /// Returns an error when the harness is revoked, mismatched, or lacks the
    /// requested capability, or when the store lock is poisoned.
    pub fn require_harness_capability(
        &self,
        ctx: &AuthContext,
        capability: HarnessCapability,
    ) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, capability, None)
    }
}
