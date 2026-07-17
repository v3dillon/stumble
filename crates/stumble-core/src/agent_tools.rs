use crate::domain::*;
use crate::ranking::{rank_discovery, RankingInput};
use crate::signing::{
    hash_api_token, new_plaintext_api_token, sign_public_event, verify_event, SigningError,
};
use crate::skill_pack::{
    default_skill_pack, export_skill_pack, fork_skill_pack, import_skill_pack, patch_skill_pack,
    pod_package_contents_from_files, source_rule_cadences, validate_pod_package_contents,
    validate_portable_package_files, validate_skill_pack, SourceRuleCadence,
};
use crate::store::{
    load_or_initialize_sqlite_store, load_sqlite_store, persist_sqlite_store_changes,
    save_store_snapshot, InMemoryStore, StoreError, StorePersistenceError,
};
use chrono::{Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use url::Url;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum AgentToolsError {
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
}

const MAX_DISCOVERY_TASK_ATTEMPTS: usize = 3;
const DEFAULT_PENDING_PROPOSAL_SECONDS: u64 = 3_600;

#[derive(Clone)]
pub struct AgentTools {
    store: Arc<RwLock<InMemoryStore>>,
    persistence: Option<Persistence>,
}

#[derive(Clone)]
enum Persistence {
    Json(Arc<PathBuf>),
    Sqlite {
        path: Arc<PathBuf>,
        baseline: Arc<Mutex<InMemoryStore>>,
    },
}

impl AgentTools {
    pub fn new(store: InMemoryStore) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            persistence: None,
        }
    }

    pub fn new_persistent(store: InMemoryStore, path: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            persistence: Some(Persistence::Json(Arc::new(path.into()))),
        }
    }

    pub fn new_sqlite_persistent(store: InMemoryStore, path: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(RwLock::new(store.clone())),
            persistence: Some(Persistence::Sqlite {
                path: Arc::new(path.into()),
                baseline: Arc::new(Mutex::new(store)),
            }),
        }
    }

    pub fn open_home_node(
        data_dir: impl AsRef<Path>,
        seed: impl FnOnce() -> InMemoryStore,
    ) -> Result<Self, AgentToolsError> {
        let database_path = data_dir.as_ref().join("stumble.sqlite3");
        let legacy_path = data_dir.as_ref().join("store.json");
        let store = load_or_initialize_sqlite_store(&database_path, &legacy_path, seed)?;
        Ok(Self::new_sqlite_persistent(store, database_path))
    }

    pub fn store(&self) -> Arc<RwLock<InMemoryStore>> {
        self.store.clone()
    }

    pub fn persistence_path(&self) -> Option<&Path> {
        match &self.persistence {
            Some(Persistence::Json(path)) => Some(path.as_path()),
            Some(Persistence::Sqlite { path, .. }) => Some(path.as_path()),
            None => None,
        }
    }

    pub fn default_auth_context(&self) -> Result<AuthContext, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.default_node()?;
        Ok(AuthContext {
            user_id: None,
            tenant_id: node.tenant_id,
            node_id: node.id,
            harness_id: None,
        })
    }

    fn persist_locked(&self, store: &mut InMemoryStore) -> Result<(), AgentToolsError> {
        match &self.persistence {
            Some(Persistence::Json(path)) => save_store_snapshot(store, path)?,
            Some(Persistence::Sqlite { path, baseline }) => {
                let mut baseline = baseline.lock().map_err(|_| AgentToolsError::LockPoisoned)?;
                if let Err(error) = persist_sqlite_store_changes(path, &baseline, store) {
                    let authoritative =
                        load_sqlite_store(path).unwrap_or_else(|_| baseline.clone());
                    *baseline = authoritative.clone();
                    *store = authoritative;
                    return Err(error.into());
                }
                *baseline = store.clone();
            }
            None => {}
        }
        Ok(())
    }

    pub fn list_pods(&self, tenant_id: Option<TenantId>) -> Result<Vec<Pod>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(store
            .pods
            .values()
            .filter(|pod| pod.tenant_id == tenant_id || pod.tenant_id.is_none())
            .cloned()
            .collect())
    }

    /// Lists only Pods visible within the caller's optional Harness Grant scope.
    ///
    /// # Errors
    ///
    /// Returns an error if the Home Node store lock is poisoned.
    pub fn list_pods_for_harness(&self, ctx: &AuthContext) -> Result<Vec<Pod>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let scoped_pod_ids =
            harness_for_context(&store, ctx)?.and_then(|harness| harness.grant.pod_ids.as_ref());
        Ok(store
            .pods
            .values()
            .filter(|pod| pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
            .filter(|pod| scoped_pod_ids.is_none_or(|pod_ids| pod_ids.contains(&pod.id)))
            .cloned()
            .collect())
    }

    /// Pods that are safe to expose on the unauthenticated federation surface.
    /// Only `Public` pods are returned; private and invite-only pods are withheld.
    pub fn list_public_pods(&self, ctx: &AuthContext) -> Result<Vec<Pod>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        harness_for_context(&store, ctx)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        Ok(store
            .pods
            .values()
            .filter(|pod| {
                (pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
                    && pod.visibility == Visibility::Public
                    && pod
                        .origin_node_id
                        .is_none_or(|origin_node_id| origin_node_id == node.id)
            })
            .cloned()
            .collect())
    }

    /// Look up a pod by slug. Thin accessor over the store.
    pub fn pod_by_slug(
        &self,
        slug: &str,
        tenant_id: Option<TenantId>,
    ) -> Result<Pod, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(store.pod_by_slug(slug, tenant_id)?)
    }

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

    pub fn route_link_to_pods(
        &self,
        ctx: &AuthContext,
        request: RouteLinkRequest,
        confidence_threshold: f32,
    ) -> Result<RouteLinkResponse, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let text = route_text(&request);
        let harness = harness_for_context(&store, ctx)?;
        let mut candidates = store
            .pods
            .values()
            .filter(|pod| pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
            .filter(|pod| {
                harness
                    .and_then(|harness| harness.grant.pod_ids.as_ref())
                    .is_none_or(|pod_ids| pod_ids.contains(&pod.id))
            })
            .map(|pod| {
                score_pod_route(
                    pod,
                    store.pod_skill_packs.get(&pod.id),
                    &text,
                    &request.tags,
                )
            })
            .collect::<Vec<_>>();
        let existing_slugs = candidates
            .iter()
            .map(|candidate| candidate.pod_slug.clone())
            .collect::<HashSet<_>>();
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let selected = candidates.first().cloned().and_then(|top| {
            let second_score = candidates
                .get(1)
                .map(|candidate| candidate.score)
                .unwrap_or(0.0);
            if top.score >= confidence_threshold && top.score - second_score >= 0.75 {
                Some(top)
            } else {
                None
            }
        });
        let needs_confirmation = selected.is_none();
        let suggested_new_pod = if needs_confirmation {
            Some(suggest_new_pod_for_link(
                &request,
                &candidates,
                &existing_slugs,
            ))
        } else {
            None
        };
        Ok(RouteLinkResponse {
            needs_confirmation,
            selected,
            candidates,
            confidence_threshold,
            suggested_new_pod,
        })
    }

    pub fn create_tenant(&self, request: CreateTenantRequest) -> Result<Tenant, AgentToolsError> {
        self.create_tenant_inner(None, request)
    }

    /// Creates a tenant on behalf of an authorized administrative harness.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the slug is duplicated,
    /// the store lock is poisoned, signing fails, or persistence fails.
    pub fn create_tenant_as(
        &self,
        ctx: &AuthContext,
        request: CreateTenantRequest,
    ) -> Result<Tenant, AgentToolsError> {
        self.create_tenant_inner(Some(ctx), request)
    }

    fn create_tenant_inner(
        &self,
        ctx: Option<&AuthContext>,
        request: CreateTenantRequest,
    ) -> Result<Tenant, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if let Some(ctx) = ctx {
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        }
        if store
            .tenants
            .values()
            .any(|tenant| tenant.slug == request.slug)
        {
            return Err(StoreError::Duplicate(format!("tenant {}", request.slug)).into());
        }
        let tenant = Tenant {
            id: Uuid::now_v7(),
            name: request.name,
            slug: request.slug,
            created_at: Utc::now(),
        };
        store.tenants.insert(tenant.id, tenant.clone());
        let node = crate::signing::create_node_identity(
            format!("{} managed node", tenant.name),
            Some(tenant.id),
        );
        store.node_identities.insert(node.id, node);
        if let Some(ctx) = ctx {
            record_harness_write(&mut store, ctx, HarnessWriteOperation::CreateTenant, None);
        }
        self.persist_locked(&mut store)?;
        Ok(tenant)
    }

    pub fn create_dev_token(
        &self,
        request: DevTokenRequest,
    ) -> Result<DevTokenResponse, AgentToolsError> {
        self.create_dev_token_inner(None, request)
    }

    /// Creates a legacy development token under administration authority.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the tenant is missing,
    /// the store lock is poisoned, or persistence fails.
    pub fn create_dev_token_as(
        &self,
        ctx: &AuthContext,
        request: DevTokenRequest,
    ) -> Result<DevTokenResponse, AgentToolsError> {
        self.create_dev_token_inner(Some(ctx), request)
    }

    fn create_dev_token_inner(
        &self,
        ctx: Option<&AuthContext>,
        request: DevTokenRequest,
    ) -> Result<DevTokenResponse, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if let Some(ctx) = ctx {
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        }
        let tenant_id = request
            .tenant_slug
            .as_deref()
            .map(|slug| store.tenant_by_slug(slug).map(|t| t.id))
            .transpose()?;
        let user_id = request.user_id.unwrap_or_else(Uuid::now_v7);
        store.users.entry(user_id).or_insert(User {
            id: user_id,
            display_name: "Remote Agent User".to_string(),
            created_at: Utc::now(),
        });
        if let Some(tenant_id) = tenant_id {
            let exists = store
                .tenant_users
                .iter()
                .any(|tu| tu.tenant_id == tenant_id && tu.user_id == user_id);
            if !exists {
                store.tenant_users.push(TenantUser {
                    tenant_id,
                    user_id,
                    role: TenantRole::Member,
                    created_at: Utc::now(),
                });
            }
        }
        let token = new_plaintext_api_token();
        let token_hash = hash_api_token(&token);
        let harness = AgentHarness {
            id: AgentHarnessId::from(Uuid::now_v7()),
            user_id,
            tenant_id,
            label: request.label.clone(),
            kind: AgentHarnessKind::Interactive,
            grant: HarnessGrant {
                capabilities: vec![],
                pod_ids: None,
            },
            created_at: Utc::now(),
            revoked_at: None,
        };
        let api_token = ApiToken {
            id: Uuid::now_v7(),
            user_id,
            tenant_id,
            token_hash: token_hash.clone(),
            label: request.label,
            created_at: Utc::now(),
            last_used_at: None,
            revoked_at: None,
            harness_id: Some(harness.id),
        };
        store.agent_harnesses.insert(harness.id, harness);
        store.api_tokens.insert(api_token.id, api_token);
        if let Some(ctx) = ctx {
            record_harness_write(&mut store, ctx, HarnessWriteOperation::CreateDevToken, None);
        }
        self.persist_locked(&mut store)?;
        Ok(DevTokenResponse {
            token,
            token_hash,
            user_id,
            tenant_id,
        })
    }

    pub fn authenticate_token(&self, token: &str) -> Result<Option<AuthContext>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let hash = hash_api_token(token);
        let Some((token_id, user_id, tenant_id, harness_id)) = store
            .api_tokens
            .iter()
            .find(|(_, token)| token.token_hash == hash && token.revoked_at.is_none())
            .filter(|(_, token)| {
                token.harness_id.is_none_or(|harness_id| {
                    store
                        .agent_harnesses
                        .get(&harness_id)
                        .is_some_and(|harness| harness.revoked_at.is_none())
                })
            })
            .map(|(id, token)| (*id, token.user_id, token.tenant_id, token.harness_id))
        else {
            return Ok(None);
        };
        let node = store.node_for_tenant(tenant_id)?;
        let api_token = store
            .api_tokens
            .get_mut(&token_id)
            .ok_or_else(|| StoreError::NotFound("api token".to_string()))?;
        api_token.last_used_at = Some(Utc::now());
        self.persist_locked(&mut store)?;
        Ok(Some(AuthContext {
            user_id: Some(user_id),
            tenant_id,
            node_id: node.id,
            harness_id,
        }))
    }

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
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        if request.label.trim().is_empty() {
            return Err(StoreError::Validation(
                "Agent Harness label must not be empty".to_string(),
            )
            .into());
        }
        let user_id = ctx
            .user_id
            .or_else(|| store.users.keys().next().copied())
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
                if store
                    .trusted_peers
                    .values()
                    .any(|peer| peer.tenant_id == ctx.tenant_id && peer.base_url == *base_url)
                {
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
                            "display_name": display_name,
                            "base_url": base_url,
                            "public_key": public_key,
                            "trust_level": TrustLevel::ReadOnly,
                            "enabled": true,
                        }),
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
            if matches!(self.persistence, Some(Persistence::Json(_))) {
                *store = before_approval;
            }
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

    /// Creates one idempotent task for every scheduled Source Rule due in the current period.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization, package parsing, locking, or persistence fails.
    pub fn materialize_due_discovery_tasks(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<DiscoveryTask>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, None)?;
        let scoped = harness_for_context(&store, ctx)?
            .and_then(|harness| harness.grant.pod_ids.as_ref())
            .cloned();
        let packages = store
            .pod_skill_packs
            .values()
            .filter(|package| {
                scoped
                    .as_ref()
                    .is_none_or(|pods| pods.contains(&package.pod_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut created = Vec::new();
        for package in packages {
            let version = PackageVersion::new(package.version)
                .map_err(|error| StoreError::Validation(error.to_string()))?;
            for (source_rule_index, cadence) in source_rule_cadences(&package.sources_yaml)
                .map_err(|error| StoreError::Validation(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                if cadence == SourceRuleCadence::OnDemand {
                    continue;
                }
                let due_at = cadence.period_start(now);
                let exists = store.discovery_tasks.values().any(|task| {
                    matches!(task.origin, DiscoveryTaskOrigin::Scheduled { source_rule_index: index } if index == source_rule_index)
                        && task.pod_id == package.pod_id
                        && task.package_version == version
                        && task.due_at == due_at
                });
                if exists {
                    continue;
                }
                let task = DiscoveryTask {
                    id: Uuid::now_v7().into(),
                    pod_id: package.pod_id,
                    package_version: version,
                    origin: DiscoveryTaskOrigin::Scheduled { source_rule_index },
                    due_at,
                    state: DiscoveryTaskState::Pending,
                    attempts: Vec::new(),
                    created_at: now,
                };
                store.discovery_tasks.insert(task.id, task.clone());
                record_harness_write(
                    &mut store,
                    ctx,
                    HarnessWriteOperation::CreateDiscoveryTask,
                    Some(task.pod_id),
                );
                created.push(task);
            }
        }
        self.persist_locked(&mut store)?;
        Ok(created)
    }

    /// Creates immediate conversational discovery work through the task contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or Package is missing, authorization is denied,
    /// or locking or persistence fails.
    pub fn create_immediate_discovery_task(
        &self,
        ctx: &AuthContext,
        request: CreateImmediateDiscoveryTaskRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::DiscoveryTasks,
            Some(request.pod_id),
        )?;
        let requested_by = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
            reason: "immediate tasks require an Agent Harness".into(),
        })?;
        if request.instructions.trim().is_empty() || request.idempotency_key.trim().is_empty() {
            return Err(StoreError::Validation(
                "immediate task instructions and idempotency key must not be empty".into(),
            )
            .into());
        }
        if let Some(existing) = store.discovery_tasks.values().find(|task| {
            matches!(&task.origin,
            DiscoveryTaskOrigin::Immediate { idempotency_key, requested_by: creator, .. }
                if creator == &requested_by && idempotency_key == &request.idempotency_key)
        }) {
            return Ok(existing.clone());
        }
        let package = store
            .pod_skill_packs
            .get(&request.pod_id)
            .ok_or_else(|| StoreError::NotFound("Pod Package".into()))?;
        let task = DiscoveryTask {
            id: Uuid::now_v7().into(),
            pod_id: request.pod_id,
            package_version: PackageVersion::new(package.version)
                .map_err(|error| StoreError::Validation(error.to_string()))?,
            origin: DiscoveryTaskOrigin::Immediate {
                instructions: request.instructions,
                idempotency_key: request.idempotency_key,
                requested_by,
            },
            due_at: now,
            state: DiscoveryTaskState::Pending,
            attempts: Vec::new(),
            created_at: now,
        };
        store.discovery_tasks.insert(task.id, task.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreateDiscoveryTask,
            Some(request.pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(task)
    }

    /// Lists visible tasks, presenting expired leases as pending work.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied or the store lock is poisoned.
    pub fn list_discovery_tasks(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<DiscoveryTask>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, None)?;
        let scoped =
            harness_for_context(&store, ctx)?.and_then(|harness| harness.grant.pod_ids.as_ref());
        Ok(store
            .discovery_tasks
            .values()
            .filter(|task| scoped.is_none_or(|pods| pods.contains(&task.pod_id)))
            .cloned()
            .map(|task| task_with_expired_lease_recorded(task, now))
            .collect())
    }

    /// Lists only tasks that can be claimed now, including safely expired leases.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied or the store lock is poisoned.
    pub fn list_ready_discovery_tasks(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<DiscoveryTask>, AgentToolsError> {
        Ok(self
            .list_discovery_tasks(ctx, now)?
            .into_iter()
            .filter(|task| task.state == DiscoveryTaskState::Pending && task.due_at <= now)
            .collect())
    }

    /// Returns one visible task and its retry history.
    ///
    /// # Errors
    ///
    /// Returns an error when the task is missing, authorization is denied, or the
    /// store lock is poisoned.
    pub fn discovery_task_status(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let task = store
            .discovery_tasks
            .get(&task_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::DiscoveryTasks,
            Some(task.pod_id),
        )?;
        Ok(task_with_expired_lease_recorded(task.clone(), now))
    }

    /// Claims pending or safely expired work for one positive lease duration.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid durations, missing or terminal tasks, active
    /// competing leases, denied authorization, or persistence failures.
    pub fn claim_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        lease_duration: DiscoveryLeaseSeconds,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) = authorized_discovery_task_mutation(&store, ctx, task_id)?;
        let expires_at = now
            .checked_add_signed(lease_duration.as_duration())
            .ok_or_else(|| {
                StoreError::Validation(
                    "lease expiration is outside the supported time range".into(),
                )
            })?;
        let task = store
            .discovery_tasks
            .get_mut(&task_id)
            .expect("BUG: task exists after lookup");
        record_expired_lease(task, now);
        if matches!(
            task.state,
            DiscoveryTaskState::Completed | DiscoveryTaskState::TerminalFailure
        ) {
            return Err(AgentToolsError::TaskTerminal);
        }
        if matches!(&task.state, DiscoveryTaskState::Leased(lease) if lease.expires_at > now) {
            return Err(AgentToolsError::TaskLeaseConflict);
        }
        task.state = DiscoveryTaskState::Leased(DiscoveryTaskLease {
            harness_id,
            claimed_at: now,
            expires_at,
        });
        let result = task.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ClaimDiscoveryTask,
            Some(pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Extends an active lease owned by the calling harness.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid durations, missing tasks, absent or foreign
    /// leases, denied authorization, or persistence failures.
    pub fn renew_discovery_task_lease(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        lease_duration: DiscoveryLeaseSeconds,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) = authorized_discovery_task_mutation(&store, ctx, task_id)?;
        let expires_at = now
            .checked_add_signed(lease_duration.as_duration())
            .ok_or_else(|| {
                StoreError::Validation(
                    "lease expiration is outside the supported time range".into(),
                )
            })?;
        let task = store
            .discovery_tasks
            .get_mut(&task_id)
            .expect("BUG: task exists after lookup");
        let DiscoveryTaskState::Leased(lease) = &mut task.state else {
            return Err(AgentToolsError::TaskLeaseRequired);
        };
        if lease.harness_id != harness_id || lease.expires_at <= now {
            return Err(AgentToolsError::TaskLeaseRequired);
        }
        lease.expires_at = expires_at;
        let result = task.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RenewDiscoveryTaskLease,
            Some(pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Completes an actively leased task and records its successful attempt.
    ///
    /// # Errors
    ///
    /// Returns an error when the task or caller-owned lease is missing,
    /// authorization is denied, or persistence fails.
    pub fn complete_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        self.finish_discovery_task(ctx, task_id, now, None)
    }

    /// Fails an actively leased task, making it retryable or terminal by history.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty reason, missing task or caller-owned lease,
    /// denied authorization, or persistence failure.
    pub fn fail_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        reason: String,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        if reason.trim().is_empty() {
            return Err(StoreError::Validation("failure reason must not be empty".into()).into());
        }
        self.finish_discovery_task(ctx, task_id, now, Some(reason))
    }

    fn finish_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        failure: Option<String>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) = authorized_discovery_task_mutation(&store, ctx, task_id)?;
        let task = store
            .discovery_tasks
            .get_mut(&task_id)
            .expect("BUG: task exists after lookup");
        let DiscoveryTaskState::Leased(lease) = &task.state else {
            return Err(AgentToolsError::TaskLeaseRequired);
        };
        if lease.harness_id != harness_id || lease.expires_at <= now {
            return Err(AgentToolsError::TaskLeaseRequired);
        }
        let lease = lease.clone();
        let outcome = if let Some(reason) = failure {
            DiscoveryTaskAttemptOutcome::Failed { reason }
        } else {
            DiscoveryTaskAttemptOutcome::Completed
        };
        task.attempts.push(DiscoveryTaskAttempt {
            harness_id,
            started_at: lease.claimed_at,
            finished_at: now,
            outcome,
        });
        task.state = if matches!(
            task.attempts.last().map(|attempt| &attempt.outcome),
            Some(DiscoveryTaskAttemptOutcome::Completed)
        ) {
            DiscoveryTaskState::Completed
        } else if task.attempts.len() >= MAX_DISCOVERY_TASK_ATTEMPTS {
            DiscoveryTaskState::TerminalFailure
        } else {
            DiscoveryTaskState::Pending
        };
        let result = task.clone();
        let operation = if result.state == DiscoveryTaskState::Completed {
            HarnessWriteOperation::CompleteDiscoveryTask
        } else {
            HarnessWriteOperation::FailDiscoveryTask
        };
        record_harness_write(&mut store, ctx, operation, Some(pod_id));
        self.persist_locked(&mut store)?;
        Ok(result)
    }

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
        self.create_pod_immediately(ctx, request)
    }

    #[cfg(test)]
    pub(crate) fn create_pod_for_test(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
    ) -> Result<Pod, AgentToolsError> {
        self.create_pod_immediately(ctx, request)
    }

    fn create_pod_immediately(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
    ) -> Result<Pod, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
        if store
            .pods
            .values()
            .any(|pod| pod.slug == request.slug && pod.tenant_id == ctx.tenant_id)
        {
            return Err(StoreError::Duplicate(format!("pod {}", request.slug)).into());
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let pod = Pod {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            name: request.name,
            slug: request.slug,
            description: request.description,
            visibility: request.visibility,
            created_by: ctx.user_id,
            created_at: Utc::now(),
            origin_node_id: Some(node.id),
        };
        store.pods.insert(pod.id, pod.clone());
        store.pod_rules.insert(
            pod.id,
            PodRules {
                pod_id: pod.id,
                blocked_topics: vec![],
                blocked_domains: vec![],
                auto_promote_crawler_candidates: false,
                federate_sources: matches!(pod.visibility, Visibility::Public),
            },
        );
        let mut package = default_skill_pack(&pod);
        package.proposer_harness_id = ctx.harness_id;
        store.insert_pod_package_version(package.clone())?;
        store.pod_skill_packs.insert(pod.id, package.clone());
        if let Some(user_id) = ctx.user_id {
            store.pod_memberships.push(PodMembership {
                user_id,
                pod_id: pod.id,
                role: PodRole::Owner,
                created_at: Utc::now(),
            });
        }
        let event = sign_public_event(
            &node,
            "pod_created",
            &pod.slug,
            json!({"pod": pod.clone(), "package": package}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreatePod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pod)
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
        let validation = validate_pod_package_contents(&request.package);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
        authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PackageManagement)?;
        if store
            .pods
            .values()
            .any(|pod| pod.slug == request.slug && pod.tenant_id == ctx.tenant_id)
        {
            return Err(StoreError::Duplicate(format!("pod {}", request.slug)).into());
        }
        let owner_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("private Pod Package requires an owner".to_string())
        })?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let now = Utc::now();
        let pod = Pod {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            name: request.name,
            slug: request.slug,
            description: request.description,
            visibility: Visibility::Private,
            created_by: Some(owner_id),
            created_at: now,
            origin_node_id: Some(node.id),
        };
        let package = PodSkillPack {
            id: Uuid::now_v7(),
            pod_id: pod.id,
            version: 1,
            context_md: request.package.context_md,
            pod_yaml: format!(
                "name: {}\nslug: {}\ndescription: {}\nvisibility: private\n",
                pod.name, pod.slug, pod.description
            ),
            skill_md: request.package.skill_md,
            sources_yaml: request.package.sources_yaml,
            filters_yaml: request.package.filters_yaml,
            examples_good_md: request.package.examples_good_md,
            examples_bad_md: request.package.examples_bad_md,
            owner_id: Some(owner_id),
            proposer_harness_id: ctx.harness_id,
            created_at: now,
            updated_at: now,
        };
        store.pods.insert(pod.id, pod.clone());
        store.pod_rules.insert(
            pod.id,
            PodRules {
                pod_id: pod.id,
                blocked_topics: Vec::new(),
                blocked_domains: Vec::new(),
                auto_promote_crawler_candidates: false,
                federate_sources: false,
            },
        );
        store.pod_memberships.push(PodMembership {
            user_id: owner_id,
            pod_id: pod.id,
            role: PodRole::Owner,
            created_at: now,
        });
        store.insert_pod_package_version(package.clone())?;
        store.pod_skill_packs.insert(pod.id, package.clone());
        let event = sign_public_event(
            &node,
            "private_pod_package_created",
            &pod.slug,
            json!({"pod": pod, "package": package}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreatePod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(CreatedPodPackage { pod, package })
    }

    pub fn join_pod(&self, ctx: &AuthContext, pod_slug: &str) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod.id),
        )?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        if !store
            .pod_memberships
            .iter()
            .any(|m| m.user_id == user_id && m.pod_id == pod.id)
        {
            store.pod_memberships.push(PodMembership {
                user_id,
                pod_id: pod.id,
                role: PodRole::Member,
                created_at: Utc::now(),
            });
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::JoinPod,
                Some(pod.id),
            );
            self.persist_locked(&mut store)?;
        }
        Ok(())
    }

    pub fn submit_link_to_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        request: SubmitLinkRequest,
    ) -> Result<Submission, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::CandidateSubmission,
            Some(pod.id),
        )?;
        let canonical_url = canonicalize_url(&request.url)?;
        let domain = Url::parse(&canonical_url)
            .map_err(|e| AgentToolsError::BadUrl(e.to_string()))?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        let existing = store
            .submissions
            .values()
            .find(|s| s.canonical_url == canonical_url && s.tenant_id == ctx.tenant_id)
            .cloned();
        let submission = if let Some(existing) = existing {
            existing
        } else {
            let description = request.description.clone();
            let submission = Submission {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                url: request.url.clone(),
                canonical_url: canonical_url.clone(),
                title: request.title.unwrap_or_else(|| canonical_url.clone()),
                description,
                domain,
                submitted_by: ctx.user_id,
                discovered_by_crawler: request.discovered_by_crawler,
                submitter_note: request.note,
                summary: request.description,
                tags: request.tags,
                embedding: None,
                created_at: Utc::now(),
                origin_event_id: None,
            };
            store.submissions.insert(submission.id, submission.clone());
            submission
        };
        if !store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod.id && link.submission_id == submission.id)
        {
            store.submission_pods.push(SubmissionPod {
                submission_id: submission.id,
                pod_id: pod.id,
                created_at: Utc::now(),
            });
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "link_submitted",
            &pod.slug,
            json!({"submission": submission.clone()}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SubmitLinkToPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(submission)
    }

    /// Authenticates, validates, canonicalizes, and privately records external discovery evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization, task ownership, input validation,
    /// idempotency, canonicalization, persistence, or locking fails.
    pub fn submit_candidate(
        &self,
        ctx: &AuthContext,
        request: CandidateSubmissionRequest,
    ) -> Result<SubmittedCandidate, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness_id = ctx
            .harness_id
            .ok_or(AgentToolsError::CandidateHarnessRequired)?;
        let harness =
            harness_for_context(&store, ctx)?.ok_or(AgentToolsError::CandidateHarnessRequired)?;
        validate_candidate_submission(&store, ctx, &request)?;

        if let Some(existing) = idempotent_candidate_submission(&store, harness_id, &request)? {
            let candidate = store
                .candidates
                .get(&existing.candidate_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
            return Ok(SubmittedCandidate {
                candidate,
                submission: existing,
                allowed_actions: vec![CandidateAllowedAction::InspectCandidate],
            });
        }
        validate_candidate_task_context(&store, ctx, harness, &request)?;

        let canonical_url = canonicalize_url(&request.evidence.source_url)?;
        let candidate = store
            .candidates
            .values()
            .find(|candidate| {
                candidate.tenant_id == ctx.tenant_id && candidate.canonical_url == canonical_url
            })
            .cloned()
            .unwrap_or_else(|| Candidate {
                id: stable_candidate_uuid(
                    "candidate",
                    &[
                        &ctx.tenant_id
                            .map_or_else(|| "local".into(), |id| id.to_string()),
                        &canonical_url,
                    ],
                )
                .into(),
                tenant_id: ctx.tenant_id,
                source_url: request.evidence.source_url.clone(),
                canonical_url,
                review_state: CandidateReviewState::Pending,
                created_at: Utc::now(),
            });
        store.candidates.insert(candidate.id, candidate.clone());

        let submission = CandidateSubmission {
            id: stable_candidate_uuid(
                "candidate-submission",
                &[
                    &harness_id.to_string(),
                    &request.evidence.harness_idempotency_key,
                    &request.evidence.client_idempotency_key,
                ],
            )
            .into(),
            candidate_id: candidate.id,
            tenant_id: ctx.tenant_id,
            submitted_by: harness_id,
            evidence: request.evidence,
            created_at: Utc::now(),
        };
        store
            .candidate_submissions
            .insert(submission.id, submission.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SubmitCandidate,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(SubmittedCandidate {
            candidate,
            submission,
            allowed_actions: vec![CandidateAllowedAction::InspectCandidate],
        })
    }

    /// Inspects a private Candidate and all independently retained evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate is missing, outside the caller's
    /// tenant or Pod scope, or the Home Node lock is poisoned.
    pub fn inspect_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
    ) -> Result<CandidateInspection, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let mut submissions: Vec<_> = store
            .candidate_submissions
            .values()
            .filter(|submission| submission.candidate_id == candidate_id)
            .cloned()
            .collect();
        for submission in &submissions {
            for placement in &submission.evidence.proposed_placements {
                authorize_harness(
                    &store,
                    ctx,
                    HarnessCapability::CandidateSubmission,
                    Some(placement.pod_id),
                )?;
            }
        }
        submissions.sort_by_key(|submission| (submission.created_at, submission.id));
        let allowed_actions = if harness_for_context(&store, ctx)?.is_some() {
            vec![CandidateAllowedAction::SubmitCandidateEvidence]
        } else {
            Vec::new()
        };
        Ok(CandidateInspection {
            candidate,
            submissions,
            allowed_actions,
        })
    }

    /// Remove a submission's association with a pod. If no pod references the
    /// submission afterward, the submission and its assets (plus any per-user
    /// saves/notes/history keyed on it) are purged too. Emits a signed
    /// `link_removed` event for the pod. Returns `true` if the submission itself
    /// was purged. Errors with `NotFound` if the link is not present in the pod.
    pub fn remove_submission_from_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        let store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod.id))?;
        if pod.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public-content removal requires a Pending Proposal".to_string(),
            )
            .into());
        }

        drop(store);
        self.remove_submission_from_pod_immediately(ctx, pod_slug, submission_id)
    }

    #[cfg(test)]
    pub(crate) fn remove_submission_from_pod_for_test(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        self.remove_submission_from_pod_immediately(ctx, pod_slug, submission_id)
    }

    fn remove_submission_from_pod_immediately(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod.id))?;

        let before = store.submission_pods.len();
        store
            .submission_pods
            .retain(|link| !(link.pod_id == pod.id && link.submission_id == submission_id));
        if store.submission_pods.len() == before {
            return Err(StoreError::NotFound(format!(
                "submission {submission_id} in pod {pod_slug}"
            ))
            .into());
        }

        // Purge the submission entirely once no pod references it anymore.
        let purged = !store
            .submission_pods
            .iter()
            .any(|link| link.submission_id == submission_id);
        if purged {
            store.submissions.remove(&submission_id);
            store
                .submission_assets
                .retain(|_, asset| asset.submission_id != submission_id);
            store.saves.retain(|(_, sid)| *sid != submission_id);
            store
                .reading_history
                .retain(|(_, sid)| *sid != submission_id);
            store
                .private_notes
                .retain(|(_, sid), _| *sid != submission_id);
        }

        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "link_removed",
            &pod.slug,
            json!({"submission_id": submission_id, "submission_purged": purged}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RemoveSubmissionFromPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(purged)
    }

    /// Routes Pod Placement removal through the sensitive-change policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or placement is missing, authorization is
    /// denied, proposal validation fails, or persistence fails.
    pub fn request_remove_submission_from_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
        now: chrono::DateTime<Utc>,
    ) -> Result<RemoveSubmissionOutcome, AgentToolsError> {
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        if pod.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::RemovePublicSubmissionFromPod {
                            pod_id: pod.id,
                            submission_id,
                        },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| RemoveSubmissionOutcome::PendingApproval(Box::new(proposal)));
        }
        self.remove_submission_from_pod(ctx, pod_slug, submission_id)
            .map(|submission_purged| RemoveSubmissionOutcome::Removed { submission_purged })
    }

    pub fn add_submission_asset(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
        request: RepresentativeImageRequest,
    ) -> Result<SubmissionAsset, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        for pod_id in store
            .submission_pods
            .iter()
            .filter(|placement| placement.submission_id == submission_id)
            .map(|placement| placement.pod_id)
        {
            authorize_harness_pod_scope(&store, ctx, pod_id)?;
        }
        let pod_ids = store
            .submission_pods
            .iter()
            .filter(|placement| placement.submission_id == submission_id)
            .map(|placement| placement.pod_id)
            .collect::<Vec<_>>();
        for pod_id in &pod_ids {
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::CandidateSubmission,
                Some(*pod_id),
            )?;
        }
        if request.url.is_none() && request.local_path.is_none() {
            return Err(StoreError::Validation(
                "representative image requires url or local_path".to_string(),
            )
            .into());
        }
        if let Some(existing) = store.submission_assets.values().find(|asset| {
            asset.submission_id == submission_id
                && asset.asset_type == SubmissionAssetType::RepresentativeImage
                && asset.source == request.source
                && asset.url == request.url
                && asset.local_path == request.local_path
        }) {
            return Ok(existing.clone());
        }
        let asset = SubmissionAsset {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            submission_id,
            asset_type: SubmissionAssetType::RepresentativeImage,
            source: request.source,
            url: request.url,
            local_path: request.local_path,
            mime_type: request.mime_type,
            alt_text: request.alt_text,
            created_at: Utc::now(),
        };
        store.submission_assets.insert(asset.id, asset.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::AddSubmissionAsset,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(asset)
    }

    pub fn assets_for_submission(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
    ) -> Result<Vec<SubmissionAsset>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        authorize_harness_submission_scope(&store, ctx, submission_id)?;
        Ok(store
            .submission_assets
            .values()
            .filter(|asset| asset.submission_id == submission_id)
            .cloned()
            .collect())
    }

    pub fn discover_in_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        request: DiscoverRequest,
    ) -> Result<Vec<DiscoveryItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, Some(pod.id))?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let submissions = store.submissions_for_pod(pod.id);
        let user_id = effective_user_id(ctx, request.user_id);
        let preferences = user_id.and_then(|id| store.user_preferences.get(&(id, ctx.tenant_id)));
        let feedback = store
            .feedback_events
            .iter()
            .filter(|f| user_id.is_some_and(|id| f.user_id == id) && f.tenant_id == ctx.tenant_id)
            .collect();
        Ok(rank_discovery(RankingInput {
            pod: &pod,
            rules: store.pod_rules.get(&pod.id),
            skill_pack: pack,
            submissions,
            preferences,
            feedback,
            query: &request.query,
            avoid: &request.avoid,
            mode: request.mode,
            limit: request.limit,
        }))
    }

    /// Lists private briefs visible to the caller's User and Harness Grant.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, a brief falls outside the
    /// harness Pod scope, or the store lock is poisoned.
    pub fn list_briefs_for_harness(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<Brief>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let is_harness = ctx.harness_id.is_some();
        let mut briefs = Vec::new();
        'briefs: for brief in store
            .briefs
            .values()
            .filter(|brief| brief.tenant_id == ctx.tenant_id)
            .filter(|brief| !is_harness || brief.user_id == ctx.user_id)
        {
            for item in &brief.items {
                match authorize_harness_submission_scope(&store, ctx, item.submission_id) {
                    Ok(()) => {}
                    Err(AgentToolsError::Forbidden { .. }) => continue 'briefs,
                    Err(error) => return Err(error),
                }
            }
            briefs.push(brief.clone());
        }
        Ok(briefs)
    }

    pub fn generate_brief(
        &self,
        ctx: &AuthContext,
        request: GenerateBriefRequest,
    ) -> Result<Brief, AgentToolsError> {
        let user_id = effective_user_id(ctx, request.user_id);
        let query = request
            .query
            .clone()
            .unwrap_or_else(|| "daily brief".to_string());
        let mut all_items = Vec::new();
        for slug in &request.pod_slugs {
            let mut items = self.discover_in_pod(
                ctx,
                slug,
                DiscoverRequest {
                    query: query.clone(),
                    avoid: vec![],
                    limit: 4,
                    mode: DiscoveryMode::DeepMatch,
                    user_id,
                },
            )?;
            all_items.append(&mut items);
        }
        all_items = self.filter_brief_candidates(ctx, user_id, all_items)?;
        all_items.truncate(4);
        let roles = [
            "one thing to read",
            "one thing to explore",
            "one older gem",
            "one adjacent surprise",
        ];
        let brief_items = all_items
            .iter()
            .enumerate()
            .map(|(idx, item)| BriefItem {
                submission_id: item.submission_id,
                role: roles.get(idx).unwrap_or(&"recommended").to_string(),
                title: item.title.clone(),
                url: item.url.clone(),
                summary: item.short_summary.clone(),
                why_it_matters: item.why_belongs_in_pod.clone(),
                why_user_may_care: item.why_matches_request.clone(),
            })
            .collect();
        let brief = Brief {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            user_id,
            title: "Stumble Brief".to_string(),
            query: request.query,
            created_at: Utc::now(),
            private: true,
            items: brief_items,
            reflection: Some(
                "What would be useful to try, not just interesting to read?".to_string(),
            ),
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        store.briefs.insert(brief.id, brief.clone());
        record_harness_write(&mut store, ctx, HarnessWriteOperation::GenerateBrief, None);
        self.persist_locked(&mut store)?;
        Ok(brief)
    }

    fn filter_brief_candidates(
        &self,
        ctx: &AuthContext,
        user_id: Option<UserId>,
        items: Vec<DiscoveryItem>,
    ) -> Result<Vec<DiscoveryItem>, AgentToolsError> {
        let Some(user_id) = user_id else {
            return Ok(items);
        };
        let stale_before = Utc::now() - Duration::days(30);
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let recently_briefed_own_links = store
            .briefs
            .values()
            .filter(|brief| {
                brief.tenant_id == ctx.tenant_id
                    && brief.user_id == Some(user_id)
                    && brief.created_at >= stale_before
            })
            .flat_map(|brief| brief.items.iter().map(|item| item.submission_id))
            .collect::<HashSet<_>>();

        Ok(items
            .into_iter()
            .filter(|item| {
                let Some(submission) = store.submissions.get(&item.submission_id) else {
                    return true;
                };
                if submission.submitted_by != Some(user_id) {
                    return true;
                }
                submission.created_at < stale_before
                    && !recently_briefed_own_links.contains(&item.submission_id)
            })
            .collect())
    }

    pub fn get_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()).into())
    }

    /// Reads one immutable historical Pod Package version.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or version does not exist, the Harness is
    /// outside its Pod scope, or the store lock is poisoned.
    pub fn get_pod_package_version(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        version: PackageVersion,
    ) -> Result<PodPackage, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        store
            .pod_package_version(pod.id, version)
            .cloned()
            .ok_or_else(|| {
                StoreError::NotFound(format!("Pod Package version {}", version.value())).into()
            })
    }

    pub fn pod_agent_context(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodAgentContext, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let validation = validate_skill_pack(&pack);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }
        Ok(PodAgentContext {
            pod_slug: pod.slug,
            pod_name: pod.name,
            skill_pack_version: pack.version,
            skill_md: pack.skill_md,
            pod_yaml: pack.pod_yaml,
            filters_yaml: pack.filters_yaml,
            validation,
        })
    }

    pub fn patch_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        patch: SkillPackPatch,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let mut pack = patch_skill_pack(&existing, patch);
        let validation = validate_skill_pack(&pack);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }
        let now = Utc::now();
        pack.created_at = now;
        pack.updated_at = now;
        pack.proposer_harness_id = ctx.harness_id;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_skill_pack_updated",
            &pod.slug,
            json!({"package": pack}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.insert_pod_package_version(pack.clone())?;
        store.pod_skill_packs.insert(pod.id, pack.clone());
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::PatchSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pack)
    }

    pub fn export_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<ExportedSkillPack, AgentToolsError> {
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
        let events_jsonl = store
            .public_events_for_pod(&pod.slug)
            .into_iter()
            .map(|event| serde_json::to_string(&event))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default()
            .join("\n");
        Ok(export_skill_pack(pack, events_jsonl))
    }

    pub fn import_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        files: BTreeMap<String, String>,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        validate_portable_package_files(&files)?;
        verify_portable_package_history(&store, &files)?;
        let mut pack = import_skill_pack(&existing, &files);
        let report = validate_skill_pack(&pack);
        if !report.valid {
            return Err(StoreError::Validation(report.errors.join(", ")).into());
        }
        let now = Utc::now();
        pack.created_at = now;
        pack.updated_at = now;
        pack.proposer_harness_id = ctx.harness_id;
        store.insert_pod_package_version(pack.clone())?;
        store.pod_skill_packs.insert(pod.id, pack.clone());
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_package_imported",
            &pod.slug,
            json!({"package": pack}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ImportSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pack)
    }

    pub fn fork_skill_pack(
        &self,
        ctx: &AuthContext,
        source_pod_slug: &str,
        target: CreatePodRequest,
    ) -> Result<PodSkillPack, AgentToolsError> {
        if target.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public Package Revisions require Pending Proposal approval".to_string(),
            )
            .into());
        }
        let source_pack = self.get_skill_pack(ctx, source_pod_slug)?;
        let target_pod = self.create_pod(ctx, target)?;
        let mut forked = fork_skill_pack(&source_pack, &target_pod);
        forked.proposer_harness_id = ctx.harness_id;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        forked.version = 2;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_package_forked",
            &target_pod.slug,
            json!({"package": forked}),
            store.latest_event_hash(&target_pod.slug),
        )?;
        store.insert_pod_package_version(forked.clone())?;
        store.pod_skill_packs.insert(target_pod.id, forked.clone());
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ForkSkillPack,
            Some(target_pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(forked)
    }

    pub fn validate_pod_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<ValidationReport, AgentToolsError> {
        let pack = self.get_skill_pack(ctx, pod_slug)?;
        Ok(validate_skill_pack(&pack))
    }

    pub fn add_source_to_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        source_type: CrawlerSourceType,
        url: String,
    ) -> Result<CrawlerSource, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, Some(pod.id))?;
        let source = CrawlerSource {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            pod_id: pod.id,
            source_type,
            url,
            enabled: true,
            crawl_interval_minutes: 1440,
            last_crawled_at: None,
            origin_event_id: None,
        };
        store.crawler_sources.insert(source.id, source.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::AddSourceToPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(source)
    }

    pub fn create_crawl_candidate(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        source_id: Uuid,
        request: SubmitLinkRequest,
    ) -> Result<CrawlCandidate, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, Some(pod.id))?;
        let canonical_url = canonicalize_url(&request.url)?;
        let domain = Url::parse(&canonical_url)
            .map_err(|e| AgentToolsError::BadUrl(e.to_string()))?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        let candidate = CrawlCandidate {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            pod_id: pod.id,
            crawler_source_id: source_id,
            url: request.url,
            canonical_url,
            title: request
                .title
                .unwrap_or_else(|| "Untitled crawler candidate".to_string()),
            description: request.description,
            domain,
            summary: None,
            tags: request.tags,
            status: CrawlCandidateStatus::Pending,
            rejection_reason: None,
            created_at: Utc::now(),
        };
        store
            .crawl_candidates
            .insert(candidate.id, candidate.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreateCrawlCandidate,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(candidate)
    }

    pub fn promote_crawl_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: Uuid,
    ) -> Result<Submission, AgentToolsError> {
        let (pod_slug, request) = {
            let mut store = self
                .store
                .write()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            let candidate = store
                .crawl_candidates
                .get(&candidate_id)
                .ok_or_else(|| StoreError::NotFound("crawl candidate".to_string()))?;
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::CandidateSubmission,
                Some(candidate.pod_id),
            )?;
            let candidate = store
                .crawl_candidates
                .get_mut(&candidate_id)
                .ok_or_else(|| StoreError::NotFound("crawl candidate".to_string()))?;
            candidate.status = CrawlCandidateStatus::Promoted;
            let candidate = candidate.clone();
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::PromoteCrawlCandidate,
                Some(candidate.pod_id),
            );
            self.persist_locked(&mut store)?;
            let pod = store
                .pods
                .get(&candidate.pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("pod".to_string()))?;
            (
                pod.slug,
                SubmitLinkRequest {
                    url: candidate.url.clone(),
                    title: Some(candidate.title.clone()),
                    description: candidate.description.clone(),
                    note: Some("Promoted from approved crawler source.".to_string()),
                    tags: candidate.tags.clone(),
                    discovered_by_crawler: true,
                },
            )
        };
        self.submit_link_to_pod(ctx, &pod_slug, request)
    }

    pub fn save_link(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Feedback, None)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        authorize_harness_submission_scope(&store, ctx, submission_id)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        store.saves.insert((user_id, submission_id));
        store.feedback_events.push(FeedbackEvent {
            user_id,
            tenant_id: ctx.tenant_id,
            submission_id,
            event_type: FeedbackKind::Saved,
            reason: None,
            created_at: Utc::now(),
            local_only: true,
        });
        record_harness_write(&mut store, ctx, HarnessWriteOperation::SaveLink, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn block_source(&self, ctx: &AuthContext, source: String) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Feedback, None)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
            });
        if !prefs.blocked_sources.contains(&source) {
            prefs.blocked_sources.push(source);
        }
        record_harness_write(&mut store, ctx, HarnessWriteOperation::BlockSource, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn block_topic(&self, ctx: &AuthContext, topic: String) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Feedback, None)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
            });
        if !prefs.blocked_topics.contains(&topic) {
            prefs.blocked_topics.push(topic);
        }
        record_harness_write(&mut store, ctx, HarnessWriteOperation::BlockTopic, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn update_preferences(
        &self,
        ctx: &AuthContext,
        request: UpdatePreferencesRequest,
    ) -> Result<UserPreferences, AgentToolsError> {
        let Some(user_id) = ctx.user_id else {
            return Err(StoreError::Validation(
                "preferences require an authenticated user".to_string(),
            )
            .into());
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Feedback, None)?;
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
            });
        if let Some(interests) = request.interests {
            prefs.interests = normalize_unique(interests);
        }
        if let Some(blocked_topics) = request.blocked_topics {
            prefs.blocked_topics = normalize_unique(blocked_topics);
        }
        if let Some(blocked_sources) = request.blocked_sources {
            prefs.blocked_sources = normalize_unique(blocked_sources);
        }
        if let Some(length) = request.preferred_brief_length {
            prefs.preferred_brief_length = length.clamp(1, 10);
        }
        if let Some(mode) = request.preferred_discovery_mode {
            prefs.preferred_discovery_mode = mode;
        }
        let prefs = prefs.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::UpdatePreferences,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(prefs)
    }

    pub fn node_info(&self, ctx: &AuthContext) -> Result<NodeInfo, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        harness_for_context(&store, ctx)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        Ok(NodeInfo {
            node_id: node.id,
            display_name: node.display_name,
            public_key: node.public_key,
            supported_protocol_version: "stumble/0.1".to_string(),
        })
    }

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
                    display_name,
                    base_url,
                    public_key,
                },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
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
        endpoints.insert(
            "hub_search_pods".to_string(),
            format!("{base}/hub/search-pods"),
        );
        Ok(WellKnownNode {
            protocol: "stumble/0.1".to_string(),
            node,
            endpoints,
        })
    }

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
            latest_known_event_hash: store.latest_event_hash(&pod.slug),
            skill_pack_version: pack.version,
            public_source_summary,
        })
    }

    pub fn register_hub_node(
        &self,
        request: HubRegisterNodeRequest,
    ) -> Result<HubRegisteredNode, AgentToolsError> {
        validate_protocol_version(&request.protocol_version)?;
        let base_url = validate_hub_base_url(&request.base_url, "base_url")?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let now = Utc::now();
        let registered = HubRegisteredNode {
            node_id: request.node_id,
            display_name: request.display_name,
            base_url: normalized_url(base_url),
            public_key: request.public_key,
            protocol_version: request.protocol_version,
            registered_at: store
                .hub_nodes
                .get(&request.node_id)
                .map(|node| node.registered_at)
                .unwrap_or(now),
            last_seen_at: now,
        };
        store
            .hub_nodes
            .insert(registered.node_id, registered.clone());
        self.persist_locked(&mut store)?;
        Ok(registered)
    }

    pub fn list_hub_nodes(&self) -> Result<Vec<HubRegisteredNode>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(store.hub_nodes.values().cloned().collect())
    }

    pub fn register_hub_pod(
        &self,
        request: HubRegisterPodRequest,
    ) -> Result<HubRegisteredPod, AgentToolsError> {
        let node_base_url = validate_hub_base_url(&request.node_base_url, "node_base_url")?;
        let manifest_url =
            validate_hub_endpoint_url(&request.manifest_url, "manifest_url", &node_base_url)?;
        let events_url =
            validate_hub_endpoint_url(&request.events_url, "events_url", &node_base_url)?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if !store.hub_nodes.contains_key(&request.node_id) {
            return Err(StoreError::Validation(format!(
                "hub pod node_id {} is not registered",
                request.node_id
            ))
            .into());
        }
        let now = Utc::now();
        let key = (request.node_id, request.pod_slug.clone());
        let registered_at = store
            .hub_pods
            .get(&key)
            .map(|pod| pod.registered_at)
            .unwrap_or(now);
        let pod = HubRegisteredPod {
            id: store
                .hub_pods
                .get(&key)
                .map(|pod| pod.id)
                .unwrap_or_else(Uuid::now_v7),
            node_id: request.node_id,
            node_base_url: normalized_url(node_base_url),
            pod_slug: request.pod_slug,
            pod_name: request.pod_name,
            description: request.description,
            tags: normalize_unique(request.tags),
            skill_pack_version: request.skill_pack_version,
            latest_event_hash: request.latest_event_hash,
            manifest_url: normalized_url(manifest_url),
            events_url: normalized_url(events_url),
            registered_at,
            updated_at: now,
        };
        store.hub_pods.insert(key, pod.clone());
        self.persist_locked(&mut store)?;
        Ok(pod)
    }

    pub fn index_local_public_pods_in_hub(
        &self,
        ctx: &AuthContext,
        base_url: &str,
    ) -> Result<Vec<HubRegisteredPod>, AgentToolsError> {
        let node_base_url = validate_hub_base_url(base_url, "base_url")?;
        let normalized_base_url = normalized_url(node_base_url.clone());
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let now = Utc::now();
        let registered_node = HubRegisteredNode {
            node_id: node.id,
            display_name: node.display_name.clone(),
            base_url: normalized_base_url.clone(),
            public_key: node.public_key.clone(),
            protocol_version: "stumble/0.1".to_string(),
            registered_at: store
                .hub_nodes
                .get(&node.id)
                .map(|node| node.registered_at)
                .unwrap_or(now),
            last_seen_at: now,
        };
        store.hub_nodes.insert(node.id, registered_node);

        let public_pods = store
            .pods
            .values()
            .filter(|pod| {
                (pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
                    && pod.visibility == Visibility::Public
                    && pod
                        .origin_node_id
                        .is_none_or(|origin_node_id| origin_node_id == node.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut indexed = Vec::new();
        for pod in public_pods {
            let Some(pack) = store.pod_skill_packs.get(&pod.id) else {
                continue;
            };
            let manifest_url = validate_hub_endpoint_url(
                &format!(
                    "{}/federation/pods/{}/manifest",
                    normalized_base_url, pod.slug
                ),
                "manifest_url",
                &node_base_url,
            )?;
            let events_url = validate_hub_endpoint_url(
                &format!(
                    "{}/federation/pods/{}/events",
                    normalized_base_url, pod.slug
                ),
                "events_url",
                &node_base_url,
            )?;
            let key = (node.id, pod.slug.clone());
            let registered_at = store
                .hub_pods
                .get(&key)
                .map(|pod| pod.registered_at)
                .unwrap_or(now);
            let registered_pod = HubRegisteredPod {
                id: store
                    .hub_pods
                    .get(&key)
                    .map(|pod| pod.id)
                    .unwrap_or_else(Uuid::now_v7),
                node_id: node.id,
                node_base_url: normalized_base_url.clone(),
                pod_slug: pod.slug.clone(),
                pod_name: pod.name.clone(),
                description: pod.description.clone(),
                tags: route_tokens(&format!("{} {} {}", pod.slug, pod.name, pod.description)),
                skill_pack_version: pack.version,
                latest_event_hash: store.latest_event_hash(&pod.slug),
                manifest_url: normalized_url(manifest_url),
                events_url: normalized_url(events_url),
                registered_at,
                updated_at: now,
            };
            store.hub_pods.insert(key, registered_pod.clone());
            indexed.push(registered_pod);
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::IndexPublicPods,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(indexed)
    }

    pub fn search_hub_pods(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<HubSearchPodsResponse, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let query = query.trim().to_lowercase();
        let query_tokens = route_tokens(&query);
        let mut results = store
            .hub_pods
            .values()
            .filter_map(|pod| score_hub_pod(pod, &query_tokens))
            .collect::<Vec<_>>();
        results.sort_by(|a, b| b.score.total_cmp(&a.score));
        results.truncate(limit.clamp(1, 50));
        Ok(HubSearchPodsResponse { query, results })
    }

    pub fn pod_discovery_feed(
        &self,
        ctx: &AuthContext,
        base_url: &str,
        query: &str,
        limit: usize,
    ) -> Result<PodDiscoveryFeedResponse, AgentToolsError> {
        self.index_local_public_pods_in_hub(ctx, base_url)?;
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let query = query.trim().to_lowercase();
        let query_tokens = route_tokens(&query);
        let mut local_public_pods = Vec::new();
        let mut global_public_pods = Vec::new();
        for pod in store.hub_pods.values() {
            let scope = if pod.node_id == node.id {
                PodDiscoveryScope::Local
            } else {
                PodDiscoveryScope::Global
            };
            let Some(item) = discovery_feed_item(pod, scope, &query_tokens) else {
                continue;
            };
            match item.scope {
                PodDiscoveryScope::Local => local_public_pods.push(item),
                PodDiscoveryScope::Global => global_public_pods.push(item),
            }
        }
        sort_discovery_feed_items(&mut local_public_pods);
        sort_discovery_feed_items(&mut global_public_pods);
        let limit = limit.clamp(1, 50);
        local_public_pods.truncate(limit);
        global_public_pods.truncate(limit);
        Ok(PodDiscoveryFeedResponse {
            query,
            local_public_pods,
            global_public_pods,
            private_interests_exported: false,
        })
    }

    pub fn discover_public_pods_for_home(
        &self,
        ctx: &AuthContext,
        topics: Vec<String>,
        limit: usize,
    ) -> Result<HomePublicPodDiscoveryResponse, AgentToolsError> {
        let mut effective_topics = normalize_unique(topics);
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if effective_topics.is_empty() {
            if let Some(user_id) = ctx.user_id {
                if let Some(prefs) = store.user_preferences.get(&(user_id, ctx.tenant_id)) {
                    effective_topics = prefs.interests.clone();
                }
            }
        }
        let query = effective_topics.join(" ");
        let route_request = RouteLinkRequest {
            url: String::new(),
            title: Some(query.clone()),
            summary: Some(query.clone()),
            tags: effective_topics.clone(),
        };
        // This is a public discovery surface. Routing scores every pod the caller
        // can see (including their own private pods), so restrict the results to
        // public slugs before returning them.
        let public_slugs: std::collections::HashSet<String> = store
            .pods
            .values()
            .filter(|pod| {
                (pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
                    && pod.visibility == Visibility::Public
            })
            .map(|pod| pod.slug.clone())
            .collect();
        drop(store);
        let local_public_pods = self
            .route_link_to_pods(ctx, route_request, 0.0)?
            .candidates
            .into_iter()
            .filter(|candidate| candidate.score > 0.0 && public_slugs.contains(&candidate.pod_slug))
            .take(limit.clamp(1, 25))
            .collect();
        let hub_results = self.search_hub_pods(&query, limit)?.results;
        Ok(HomePublicPodDiscoveryResponse {
            topics: effective_topics,
            local_public_pods,
            hub_results,
            private_interests_exported: false,
        })
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
            project_imported_public_event(&mut store, ctx, &event);
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

    pub fn import_public_events_from_hub_node(
        &self,
        ctx: &AuthContext,
        node_id: NodeIdentityId,
        events: Vec<EventLog>,
    ) -> Result<usize, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store
            .hub_nodes
            .get(&node_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("hub node {node_id}")))?;
        let mut imported = 0;
        for mut event in events {
            if event.author_node_id != node_id || crate::store::is_private_event(&event.event_type)
            {
                continue;
            }
            if store.event_log.iter().any(|existing| {
                existing.event_id == event.event_id || existing.content_hash == event.content_hash
            }) {
                continue;
            }
            if !verify_event(&event, &node.public_key)? {
                return Err(StoreError::InvalidSignature.into());
            }
            event.imported_from_peer_id = None;
            event.verified = true;
            event.tenant_id = ctx.tenant_id;
            project_imported_public_event(&mut store, ctx, &event);
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

fn task_with_expired_lease_recorded(
    mut task: DiscoveryTask,
    now: chrono::DateTime<Utc>,
) -> DiscoveryTask {
    record_expired_lease(&mut task, now);
    task
}

fn record_expired_lease(task: &mut DiscoveryTask, now: chrono::DateTime<Utc>) {
    let DiscoveryTaskState::Leased(lease) = &task.state else {
        return;
    };
    if lease.expires_at > now {
        return;
    }
    let lease = lease.clone();
    task.attempts.push(DiscoveryTaskAttempt {
        harness_id: lease.harness_id,
        started_at: lease.claimed_at,
        finished_at: lease.expires_at,
        outcome: DiscoveryTaskAttemptOutcome::LeaseExpired,
    });
    task.state = if task.attempts.len() >= MAX_DISCOVERY_TASK_ATTEMPTS {
        DiscoveryTaskState::TerminalFailure
    } else {
        DiscoveryTaskState::Pending
    };
}

pub fn canonicalize_url(value: &str) -> Result<String, AgentToolsError> {
    let mut url = Url::parse(value).map_err(|e| AgentToolsError::BadUrl(e.to_string()))?;
    url.set_fragment(None);
    if (url.scheme() == "https" && url.port() == Some(443))
        || (url.scheme() == "http" && url.port() == Some(80))
    {
        let _ = url.set_port(None);
    }
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            !matches!(
                k.as_ref(),
                "utm_source" | "utm_medium" | "utm_campaign" | "utm_term" | "utm_content"
            )
        })
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Ok(url.to_string())
}

fn project_imported_public_event(store: &mut InMemoryStore, ctx: &AuthContext, event: &EventLog) {
    match event.event_type.as_str() {
        "pod_created" => {
            let Some(pod) = event
                .payload_json
                .get("pod")
                .and_then(|value| serde_json::from_value::<Pod>(value.clone()).ok())
            else {
                return;
            };
            project_imported_pod(store, ctx, event.author_node_id, pod);
        }
        "link_submitted" => {
            let Some(submission) = event
                .payload_json
                .get("submission")
                .and_then(|value| serde_json::from_value::<Submission>(value.clone()).ok())
            else {
                return;
            };
            project_imported_submission(store, ctx, event, submission);
        }
        "link_removed" => {
            let Some(submission_id) = event
                .payload_json
                .get("submission_id")
                .and_then(|value| serde_json::from_value::<SubmissionId>(value.clone()).ok())
            else {
                return;
            };
            if let Some(pod_id) = store
                .pods
                .values()
                .find(|pod| pod.slug == event.pod_slug && pod.tenant_id == ctx.tenant_id)
                .map(|pod| pod.id)
            {
                store
                    .submission_pods
                    .retain(|link| !(link.pod_id == pod_id && link.submission_id == submission_id));
            }
        }
        _ => {}
    }
}

fn project_imported_pod(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    origin_node_id: NodeIdentityId,
    mut pod: Pod,
) -> PodId {
    pod.tenant_id = ctx.tenant_id;
    pod.visibility = Visibility::Public;
    pod.created_by = None;
    pod.origin_node_id = Some(origin_node_id);

    if let Some(existing) = store
        .pods
        .values()
        .find(|existing| existing.slug == pod.slug && existing.tenant_id == ctx.tenant_id)
        .cloned()
    {
        ensure_projected_pod_support(store, &existing);
        return existing.id;
    }

    let pod_id = pod.id;
    store.pods.insert(pod_id, pod.clone());
    ensure_projected_pod_support(store, &pod);
    pod_id
}

fn ensure_projected_pod_support(store: &mut InMemoryStore, pod: &Pod) {
    store.pod_rules.entry(pod.id).or_insert(PodRules {
        pod_id: pod.id,
        blocked_topics: vec![],
        blocked_domains: vec![],
        auto_promote_crawler_candidates: false,
        federate_sources: true,
    });
    store
        .pod_skill_packs
        .entry(pod.id)
        .or_insert_with(|| default_skill_pack(pod));
}

fn project_imported_submission(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
    mut submission: Submission,
) {
    let pod_id = store
        .pods
        .values()
        .find(|pod| pod.slug == event.pod_slug && pod.tenant_id == ctx.tenant_id)
        .map(|pod| pod.id)
        .unwrap_or_else(|| {
            let pod = Pod {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                name: event.pod_slug.clone(),
                slug: event.pod_slug.clone(),
                description: "Imported public pod from a federated node.".to_string(),
                visibility: Visibility::Public,
                created_by: None,
                created_at: event.created_at,
                origin_node_id: Some(event.author_node_id),
            };
            project_imported_pod(store, ctx, event.author_node_id, pod)
        });

    submission.tenant_id = ctx.tenant_id;
    submission.submitted_by = None;
    submission.origin_event_id = Some(event.event_id);
    let submission_id = store
        .submissions
        .values()
        .find(|existing| {
            existing.tenant_id == ctx.tenant_id
                && (existing.id == submission.id
                    || existing.canonical_url == submission.canonical_url)
        })
        .map(|existing| existing.id)
        .unwrap_or_else(|| {
            let id = submission.id;
            store.submissions.insert(id, submission);
            id
        });

    if !store
        .submission_pods
        .iter()
        .any(|link| link.pod_id == pod_id && link.submission_id == submission_id)
    {
        store.submission_pods.push(SubmissionPod {
            submission_id,
            pod_id,
            created_at: event.created_at,
        });
    }
}

fn validate_protocol_version(value: &str) -> Result<(), AgentToolsError> {
    if value == "stumble/0.1" {
        return Ok(());
    }
    Err(StoreError::Validation(format!(
        "unsupported protocol_version {value}; expected stumble/0.1"
    ))
    .into())
}

fn validate_hub_base_url(value: &str, field: &str) -> Result<Url, AgentToolsError> {
    let mut url = parse_hub_url(value, field)?;
    url.set_query(None);
    url.set_fragment(None);
    validate_hub_scheme_and_host(&url, field)?;
    Ok(url)
}

fn validate_hub_endpoint_url(
    value: &str,
    field: &str,
    node_base_url: &Url,
) -> Result<Url, AgentToolsError> {
    let mut url = parse_hub_url(value, field)?;
    url.set_query(None);
    url.set_fragment(None);
    validate_hub_scheme_and_host(&url, field)?;
    if url.scheme() != node_base_url.scheme()
        || url.host_str() != node_base_url.host_str()
        || url.port_or_known_default() != node_base_url.port_or_known_default()
    {
        return Err(StoreError::Validation(format!(
            "{field} must use the same scheme, host, and port as node_base_url"
        ))
        .into());
    }
    if !url
        .path()
        .starts_with(node_base_url.path().trim_end_matches('/'))
    {
        return Err(StoreError::Validation(format!("{field} must be under node_base_url")).into());
    }
    Ok(url)
}

fn parse_hub_url(value: &str, field: &str) -> Result<Url, AgentToolsError> {
    let url = Url::parse(value)
        .map_err(|error| StoreError::Validation(format!("{field} is not a valid URL: {error}")))?;
    if url.username() != "" || url.password().is_some() {
        return Err(StoreError::Validation(format!("{field} must not include credentials")).into());
    }
    Ok(url)
}

fn validate_hub_scheme_and_host(url: &Url, field: &str) -> Result<(), AgentToolsError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(StoreError::Validation(format!("{field} must use http or https")).into());
    }
    if url.host_str().is_none() {
        return Err(StoreError::Validation(format!("{field} must include a host")).into());
    }
    if !hub_url_is_loopback(url) && url.scheme() != "https" {
        return Err(StoreError::Validation(format!(
            "{field} must use https unless it is loopback-only"
        ))
        .into());
    }
    Ok(())
}

fn hub_url_is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn normalized_url(url: Url) -> String {
    url.to_string().trim_end_matches('/').to_string()
}

fn normalize_unique(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        let value = value.trim().to_lowercase();
        if !value.is_empty() && !output.contains(&value) {
            output.push(value);
        }
    }
    output
}

fn route_text(request: &RouteLinkRequest) -> String {
    format!(
        "{} {} {} {}",
        request.url,
        request.title.clone().unwrap_or_default(),
        request.summary.clone().unwrap_or_default(),
        request.tags.join(" ")
    )
    .to_lowercase()
}

fn suggest_new_pod_for_link(
    request: &RouteLinkRequest,
    candidates: &[PodRouteCandidate],
    existing_slugs: &HashSet<String>,
) -> CreatePodRequest {
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let tags = normalize_unique(request.tags.clone());
    let name = tags
        .first()
        .map(|tag| title_case_words(tag))
        .or_else(|| title.map(compact_title_for_pod))
        .or_else(|| domain_label(&request.url).map(|domain| title_case_words(&domain)))
        .unwrap_or_else(|| "New Links".to_string());
    let slug = unique_slug(slugify(&name), existing_slugs);
    let basis = if !tags.is_empty() {
        format!("tagged {}", tags.join(", "))
    } else if let Some(domain) = domain_label(&request.url) {
        format!("from {domain}")
    } else {
        "from submitted links".to_string()
    };
    let description = if let Some(top) = candidates.first() {
        format!(
            "User-approved links {basis}. Suggested because no existing pod cleared the routing threshold; closest match was {} with score {:.1}.",
            top.pod_name, top.score
        )
    } else {
        format!(
            "User-approved links {basis}. Suggested because there are no existing pods to route this link into."
        )
    };
    CreatePodRequest {
        name,
        slug,
        description,
        visibility: Visibility::Private,
    }
}

fn unique_slug(base: String, existing_slugs: &HashSet<String>) -> String {
    if !existing_slugs.contains(&base) {
        return base;
    }
    for idx in 2..=100 {
        let candidate = format!("{base}-{idx}");
        if !existing_slugs.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", Uuid::now_v7())
}

fn compact_title_for_pod(title: &str) -> String {
    let words = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .collect::<Vec<_>>();
    if words.is_empty() {
        "New Links".to_string()
    } else {
        title_case_words(&words.join(" "))
    }
}

fn domain_label(url: &str) -> Option<String> {
    let domain = Url::parse(url).ok()?.domain()?.to_string();
    let mut parts = domain
        .trim_start_matches("www.")
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() > 1 {
        parts.pop();
    }
    parts.last().map(|part| part.replace('-', " "))
}

fn title_case_words(value: &str) -> String {
    let words = value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(&chars.as_str().to_lowercase());
                    out
                }
                None => String::new(),
            }
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.is_empty() {
        "New Links".to_string()
    } else {
        words.join(" ")
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "new-links".to_string()
    } else {
        slug
    }
}

fn score_pod_route(
    pod: &Pod,
    pack: Option<&PodSkillPack>,
    text: &str,
    tags: &[String],
) -> PodRouteCandidate {
    let mut score = 0.0_f32;
    let mut reasons = Vec::new();
    let tag_text = tags.join(" ").to_lowercase();
    let pod_text = format!("{} {} {}", pod.name, pod.slug, pod.description).to_lowercase();
    for token in route_tokens(&pod_text) {
        if text.contains(&token) || tag_text.contains(&token) {
            score += 1.5;
            if reasons.len() < 4 {
                reasons.push(format!("matched pod term '{token}'"));
            }
        }
    }
    if let Some(pack) = pack {
        let skill_text =
            format!("{} {} {}", pack.skill_md, pack.pod_yaml, pack.filters_yaml).to_lowercase();
        for token in route_tokens(&skill_text) {
            if text.contains(&token) || tag_text.contains(&token) {
                score += 0.4;
                if reasons.len() < 6 {
                    reasons.push(format!("matched skill-pack term '{token}'"));
                }
            }
        }
    }
    let domain_bonus = if text.contains("x.com") || text.contains("twitter.com") {
        if pod.slug.contains("alien") || pod.slug.contains("internet") {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    if domain_bonus > 0.0 {
        score += domain_bonus;
        reasons.push("social link fits this pod's discovery surface".to_string());
    }
    PodRouteCandidate {
        pod_slug: pod.slug.clone(),
        pod_name: pod.name.clone(),
        score,
        reasons,
    }
}

fn score_hub_pod(pod: &HubRegisteredPod, query_tokens: &[String]) -> Option<HubSearchPodResult> {
    let haystack = format!(
        "{} {} {} {}",
        pod.pod_slug,
        pod.pod_name,
        pod.description,
        pod.tags.join(" ")
    )
    .to_lowercase();
    let mut score = 0.0_f32;
    let mut reasons = Vec::new();
    for token in query_tokens {
        if haystack.contains(token) {
            score += if pod.tags.iter().any(|tag| tag.eq_ignore_ascii_case(token)) {
                2.0
            } else {
                1.0
            };
            if reasons.len() < 6 {
                reasons.push(format!("matched public pod term '{token}'"));
            }
        }
    }
    if score <= 0.0 {
        None
    } else {
        Some(HubSearchPodResult {
            pod: pod.clone(),
            score,
            reasons,
        })
    }
}

fn discovery_feed_item(
    pod: &HubRegisteredPod,
    scope: PodDiscoveryScope,
    query_tokens: &[String],
) -> Option<PodDiscoveryFeedItem> {
    if query_tokens.is_empty() {
        return Some(PodDiscoveryFeedItem {
            pod: pod.clone(),
            scope,
            score: 0.0,
            reasons: Vec::new(),
        });
    }
    let scored = score_hub_pod(pod, query_tokens)?;
    Some(PodDiscoveryFeedItem {
        pod: scored.pod,
        scope,
        score: scored.score,
        reasons: scored.reasons,
    })
}

fn sort_discovery_feed_items(items: &mut [PodDiscoveryFeedItem]) {
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.pod.updated_at.cmp(&a.pod.updated_at))
    });
}

fn authorize_harness(
    store: &InMemoryStore,
    ctx: &AuthContext,
    capability: HarnessCapability,
    pod_id: Option<PodId>,
) -> Result<(), AgentToolsError> {
    let Some(harness) = harness_for_context(store, ctx)? else {
        return Ok(());
    };
    if !harness.grant.capabilities.contains(&capability) {
        return Err(AgentToolsError::Forbidden {
            reason: format!("harness grant lacks {capability}"),
        });
    }
    if let (Some(allowed), Some(pod_id)) = (&harness.grant.pod_ids, pod_id) {
        if !allowed.contains(&pod_id) {
            return Err(AgentToolsError::Forbidden {
                reason: format!("harness grant does not include Pod {pod_id}"),
            });
        }
    }
    Ok(())
}

fn validate_candidate_submission(
    store: &InMemoryStore,
    ctx: &AuthContext,
    request: &CandidateSubmissionRequest,
) -> Result<(), AgentToolsError> {
    let evidence = &request.evidence;
    if evidence.harness_idempotency_key.trim().is_empty()
        || evidence.client_idempotency_key.trim().is_empty()
    {
        return Err(StoreError::Validation(
            "Candidate Submission idempotency keys must not be empty".into(),
        )
        .into());
    }
    if evidence.provenance.discovery_method.trim().is_empty() {
        return Err(StoreError::Validation(
            "Candidate Submission discovery method must not be empty".into(),
        )
        .into());
    }
    if evidence.proposed_placements.is_empty() {
        return Err(StoreError::Validation(
            "Candidate Submission requires at least one proposed Pod Placement".into(),
        )
        .into());
    }
    canonicalize_url(&evidence.source_url)?;
    if let Some(referrer_url) = &evidence.provenance.referrer_url {
        canonicalize_url(referrer_url)?;
    }

    let mut pod_ids = HashSet::with_capacity(evidence.proposed_placements.len());
    let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
    for placement in &evidence.proposed_placements {
        if !pod_ids.insert(placement.pod_id) {
            return Err(StoreError::Validation(
                "Candidate Submission cannot propose the same Pod twice".into(),
            )
            .into());
        }
        if placement.reason.trim().is_empty() {
            return Err(StoreError::Validation(
                "Candidate Placement reason must not be empty".into(),
            )
            .into());
        }
        let pod = store
            .pods
            .get(&placement.pod_id)
            .ok_or_else(|| StoreError::NotFound("Pod".into()))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != local_node_id)
        {
            return Err(AgentToolsError::Forbidden {
                reason: format!(
                    "Candidate Submission cannot propose remote Pod {} as a local placement",
                    placement.pod_id
                ),
            });
        }
        authorize_harness(
            store,
            ctx,
            HarnessCapability::CandidateSubmission,
            Some(placement.pod_id),
        )?;
    }

    Ok(())
}

fn validate_candidate_task_context(
    store: &InMemoryStore,
    ctx: &AuthContext,
    harness: &AgentHarness,
    request: &CandidateSubmissionRequest,
) -> Result<(), AgentToolsError> {
    match request.evidence.task_context {
        Some(task_context) => {
            let task = store
                .discovery_tasks
                .get(&task_context.task_id)
                .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
            authorize_harness(
                store,
                ctx,
                HarnessCapability::DiscoveryTasks,
                Some(task.pod_id),
            )?;
            if task.package_version != task_context.package_version {
                return Err(AgentToolsError::CandidatePackageVersionMismatch);
            }
            if !request
                .evidence
                .proposed_placements
                .iter()
                .any(|placement| placement.pod_id == task.pod_id)
            {
                return Err(StoreError::Validation(
                    "task-driven Candidate Submission must propose its task Pod".into(),
                )
                .into());
            }
            if !matches!(
                &task.state,
                DiscoveryTaskState::Leased(lease)
                    if lease.harness_id == harness.id && lease.expires_at > Utc::now()
            ) {
                return Err(AgentToolsError::CandidateTaskLeaseRequired);
            }
        }
        None if harness.kind == AgentHarnessKind::Unattended => {
            return Err(AgentToolsError::CandidateTaskRequired)
        }
        None => {}
    }
    Ok(())
}

fn idempotent_candidate_submission(
    store: &InMemoryStore,
    harness_id: AgentHarnessId,
    request: &CandidateSubmissionRequest,
) -> Result<Option<CandidateSubmission>, AgentToolsError> {
    let matching_key = store.candidate_submissions.values().find(|submission| {
        submission.submitted_by == harness_id
            && (submission.evidence.harness_idempotency_key
                == request.evidence.harness_idempotency_key
                || submission.evidence.client_idempotency_key
                    == request.evidence.client_idempotency_key)
    });
    let Some(existing) = matching_key else {
        return Ok(None);
    };
    if candidate_submission_matches_request(existing, request) {
        Ok(Some(existing.clone()))
    } else {
        Err(AgentToolsError::CandidateIdempotencyConflict)
    }
}

fn candidate_submission_matches_request(
    submission: &CandidateSubmission,
    request: &CandidateSubmissionRequest,
) -> bool {
    submission.evidence == request.evidence
}

fn stable_candidate_uuid(namespace: &str, parts: &[&str]) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(namespace.len().to_be_bytes());
    hasher.update(namespace.as_bytes());
    for part in parts {
        hasher.update(part.len().to_be_bytes());
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn authorized_discovery_task_mutation(
    store: &InMemoryStore,
    ctx: &AuthContext,
    task_id: DiscoveryTaskId,
) -> Result<(PodId, AgentHarnessId), AgentToolsError> {
    let pod_id = store
        .discovery_tasks
        .get(&task_id)
        .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?
        .pod_id;
    authorize_harness(store, ctx, HarnessCapability::DiscoveryTasks, Some(pod_id))?;
    let harness_id = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
        reason: "task mutation requires an Agent Harness".into(),
    })?;
    Ok((pod_id, harness_id))
}

fn authorize_harness_for_new_pod(
    store: &InMemoryStore,
    ctx: &AuthContext,
    capability: HarnessCapability,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, capability, None)?;
    if let Some(harness) = ctx
        .harness_id
        .and_then(|harness_id| store.agent_harnesses.get(&harness_id))
    {
        if harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "a Pod-scoped harness grant cannot create a new Pod".to_string(),
            });
        }
    }
    Ok(())
}

fn authorize_harness_pod_scope(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> Result<(), AgentToolsError> {
    let Some(harness) = harness_for_context(store, ctx)? else {
        return Ok(());
    };
    if harness
        .grant
        .pod_ids
        .as_ref()
        .is_some_and(|pod_ids| !pod_ids.contains(&pod_id))
    {
        return Err(AgentToolsError::Forbidden {
            reason: format!("harness grant does not include Pod {pod_id}"),
        });
    }
    Ok(())
}

fn authorize_harness_submission_scope(
    store: &InMemoryStore,
    ctx: &AuthContext,
    submission_id: SubmissionId,
) -> Result<(), AgentToolsError> {
    harness_for_context(store, ctx)?;
    for pod_id in store
        .submission_pods
        .iter()
        .filter(|placement| placement.submission_id == submission_id)
        .map(|placement| placement.pod_id)
    {
        authorize_harness_pod_scope(store, ctx, pod_id)?;
    }
    Ok(())
}

fn harness_for_context<'a>(
    store: &'a InMemoryStore,
    ctx: &AuthContext,
) -> Result<Option<&'a AgentHarness>, AgentToolsError> {
    let Some(harness_id) = ctx.harness_id else {
        return Ok(None);
    };
    let harness = store
        .agent_harnesses
        .get(&harness_id)
        .filter(|harness| harness.revoked_at.is_none())
        .ok_or_else(|| AgentToolsError::Forbidden {
            reason: "harness grant is revoked or missing".to_string(),
        })?;
    if Some(harness.user_id) != ctx.user_id || harness.tenant_id != ctx.tenant_id {
        return Err(AgentToolsError::Forbidden {
            reason: "harness grant does not match the authorization context".to_string(),
        });
    }
    Ok(Some(harness))
}

fn record_harness_write(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    operation: HarnessWriteOperation,
    pod_id: Option<PodId>,
) {
    if let Some(harness_id) = ctx.harness_id {
        store.harness_write_audit.push(HarnessWriteAudit {
            id: Uuid::now_v7(),
            harness_id,
            operation,
            pod_id,
            occurred_at: Utc::now(),
        });
    }
}

fn verify_portable_package_history(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
) -> Result<(), AgentToolsError> {
    let events_text = files.get("events.jsonl").ok_or_else(|| {
        StoreError::Validation("portable Pod Package is missing events.jsonl".into())
    })?;
    let events = events_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<EventLog>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| StoreError::Validation(format!("events.jsonl is invalid: {error}")))?;
    if events.is_empty() {
        return Err(StoreError::Validation(
            "events.jsonl must contain signed package history".to_string(),
        )
        .into());
    }
    let mut previous_hash: Option<&str> = None;
    for event in &events {
        if event.previous_event_hash.as_deref() != previous_hash {
            return Err(StoreError::InvalidSignature.into());
        }
        let public_key = store
            .node_identities
            .get(&event.author_node_id)
            .filter(|node| node.tenant_id == event.tenant_id)
            .map(|node| node.public_key.as_str())
            .or_else(|| {
                store
                    .hub_nodes
                    .get(&event.author_node_id)
                    .map(|node| node.public_key.as_str())
            })
            .or_else(|| {
                store
                    .trusted_peers
                    .get(&event.author_node_id)
                    .filter(|peer| peer.enabled)
                    .map(|peer| peer.public_key.as_str())
            })
            .ok_or(StoreError::UntrustedPeer)?;
        if !verify_event(event, public_key)? {
            return Err(StoreError::InvalidSignature.into());
        }
        previous_hash = Some(&event.content_hash);
    }
    let requested = pod_package_contents_from_files(files)?;
    let has_signed_contents = events.iter().any(|event| {
        event
            .payload_json
            .get("package")
            .and_then(|value| serde_json::from_value::<PodSkillPack>(value.clone()).ok())
            .is_some_and(|package| package_contents_match(&package, &requested))
    });
    if !has_signed_contents {
        return Err(StoreError::Validation(
            "events.jsonl does not contain the signed package contents".to_string(),
        )
        .into());
    }
    Ok(())
}

fn ensure_direct_package_revision_allowed(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod: &Pod,
) -> Result<(), AgentToolsError> {
    ensure_direct_package_revision_allowed_for_origin(store, ctx, pod)?;
    if pod.visibility == Visibility::Public {
        return Err(StoreError::Validation(
            "public Package Revisions require Pending Proposal approval".to_string(),
        )
        .into());
    }
    Ok(())
}

fn ensure_direct_package_revision_allowed_for_origin(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod: &Pod,
) -> Result<(), AgentToolsError> {
    let local_node = store.node_for_tenant(ctx.tenant_id)?;
    if pod
        .origin_node_id
        .is_some_and(|origin_node_id| origin_node_id != local_node.id)
    {
        return Err(StoreError::Validation(
            "remote Pod Packages may change only through verified synchronization".to_string(),
        )
        .into());
    }
    Ok(())
}

fn package_contents_match(package: &PodSkillPack, contents: &PodPackageContents) -> bool {
    package.context_md == contents.context_md
        && package.skill_md == contents.skill_md
        && package.sources_yaml == contents.sources_yaml
        && package.filters_yaml == contents.filters_yaml
        && package.examples_good_md == contents.examples_good_md
        && package.examples_bad_md == contents.examples_bad_md
}

fn normalize_capabilities(mut capabilities: Vec<HarnessCapability>) -> Vec<HarnessCapability> {
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn authorize_proposal_reader(
    store: &InMemoryStore,
    ctx: &AuthContext,
    proposal_id: PendingProposalId,
) -> Result<(), AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    let Some(harness) = harness_for_context(store, ctx)? else {
        if ctx.tenant_id == proposal.tenant_id {
            return Ok(());
        }
        return Err(AgentToolsError::Forbidden {
            reason: "Pending Proposal belongs to another tenant".to_string(),
        });
    };
    if harness.tenant_id == proposal.tenant_id
        && harness.user_id == proposal.user_id
        && (harness.id == proposal.proposer
            || (harness
                .grant
                .capabilities
                .contains(&HarnessCapability::Approval)
                && approval_scope_allows(harness, proposal)))
    {
        return Ok(());
    }
    Err(AgentToolsError::Forbidden {
        reason: "harness cannot inspect this Pending Proposal".to_string(),
    })
}

fn authorize_independent_approver(
    store: &InMemoryStore,
    ctx: &AuthContext,
    proposal_id: PendingProposalId,
) -> Result<AgentHarnessId, AgentToolsError> {
    authorize_harness(store, ctx, HarnessCapability::Approval, None)?;
    let harness = harness_for_context(store, ctx)?.ok_or_else(|| AgentToolsError::Forbidden {
        reason: "approval requires an authenticated Agent Harness".to_string(),
    })?;
    if harness.kind != AgentHarnessKind::Interactive {
        return Err(AgentToolsError::Forbidden {
            reason: "approval requires an interactive Agent Harness".to_string(),
        });
    }
    let proposal = store
        .pending_proposals
        .get(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    if harness.tenant_id != proposal.tenant_id || harness.user_id != proposal.user_id {
        return Err(AgentToolsError::Forbidden {
            reason: "approval must belong to the proposal User and tenant".to_string(),
        });
    }
    if !approval_scope_allows(harness, proposal) {
        return Err(AgentToolsError::Forbidden {
            reason: "approval Harness Grant does not cover the affected resources".to_string(),
        });
    }
    if proposal.proposer == harness.id {
        return Err(AgentToolsError::Forbidden {
            reason: "a harness cannot approve its own Pending Proposal".to_string(),
        });
    }
    Ok(harness.id)
}

fn approval_scope_allows(harness: &AgentHarness, proposal: &PendingProposal) -> bool {
    proposal
        .affected_resources
        .iter()
        .all(|resource| match resource {
            ProposalResource::Pod(pod_id)
            | ProposalResource::PodPackage(pod_id)
            | ProposalResource::SubmissionPlacement { pod_id, .. } => harness
                .grant
                .pod_ids
                .as_ref()
                .is_none_or(|pod_ids| pod_ids.contains(pod_id)),
            ProposalResource::PodSlug(_)
            | ProposalResource::AgentHarness(_)
            | ProposalResource::TrustedPeerUrl(_) => harness.grant.pod_ids.is_none(),
        })
}

fn expire_proposal(
    store: &mut InMemoryStore,
    proposal_id: PendingProposalId,
    now: chrono::DateTime<Utc>,
) -> Result<(), AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get_mut(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    if proposal.status == ProposalStatus::Pending && now >= proposal.expires_at {
        proposal.status = ProposalStatus::Expired;
        proposal.decided_at = Some(now);
    }
    Ok(())
}

fn validate_structured_diff(
    store: &InMemoryStore,
    proposal: &PendingProposal,
) -> Result<(), AgentToolsError> {
    for difference in &proposal.structured_diff {
        let current = match &difference.resource {
            ProposalResource::Pod(pod_id) => store.pods.get(pod_id).map_or(
                serde_json::Value::Null,
                |pod| json!({"visibility": pod.visibility}),
            ),
            ProposalResource::PodSlug(slug) => store
                .pods
                .values()
                .find(|pod| pod.tenant_id == proposal.tenant_id && pod.slug == *slug)
                .map_or(serde_json::Value::Null, |pod| json!(pod)),
            ProposalResource::AgentHarness(harness_id) => store
                .agent_harnesses
                .get(harness_id)
                .map_or(serde_json::Value::Null, |harness| json!(harness.grant)),
            ProposalResource::TrustedPeerUrl(base_url) => store
                .trusted_peers
                .values()
                .find(|peer| peer.tenant_id == proposal.tenant_id && peer.base_url == *base_url)
                .map_or(serde_json::Value::Null, |peer| json!(peer)),
            ProposalResource::PodPackage(pod_id) => store
                .pod_skill_packs
                .get(pod_id)
                .map_or(serde_json::Value::Null, |package| json!(package)),
            ProposalResource::SubmissionPlacement {
                pod_id,
                submission_id,
            } => json!({
                "accepted": store.submission_pods.iter().any(|placement| {
                    placement.pod_id == *pod_id && placement.submission_id == *submission_id
                })
            }),
        };
        if current != difference.before {
            return Err(StoreError::Validation(
                "proposal structured diff is stale; create a new Pending Proposal".to_string(),
            )
            .into());
        }
    }
    Ok(())
}

fn apply_sensitive_change(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    proposer: AgentHarnessId,
    requested_change: &SensitiveChange,
) -> Result<(), AgentToolsError> {
    match requested_change {
        SensitiveChange::CreatePublicPod { request } => {
            if store
                .pods
                .values()
                .any(|pod| pod.slug == request.slug && pod.tenant_id == ctx.tenant_id)
            {
                return Err(StoreError::Duplicate(format!("pod {}", request.slug)).into());
            }
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let created_by = store
                .agent_harnesses
                .get(&proposer)
                .map(|harness| harness.user_id);
            let pod = Pod {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                name: request.name.clone(),
                slug: request.slug.clone(),
                description: request.description.clone(),
                visibility: Visibility::Public,
                created_by,
                created_at: Utc::now(),
                origin_node_id: Some(node.id),
            };
            store.pods.insert(pod.id, pod.clone());
            store.pod_rules.insert(
                pod.id,
                PodRules {
                    pod_id: pod.id,
                    blocked_topics: vec![],
                    blocked_domains: vec![],
                    auto_promote_crawler_candidates: false,
                    federate_sources: true,
                },
            );
            let mut package = default_skill_pack(&pod);
            package.proposer_harness_id = Some(proposer);
            store.insert_pod_package_version(package.clone())?;
            store.pod_skill_packs.insert(pod.id, package.clone());
            if let Some(user_id) = created_by {
                store.pod_memberships.push(PodMembership {
                    user_id,
                    pod_id: pod.id,
                    role: PodRole::Owner,
                    created_at: Utc::now(),
                });
            }
            let event = sign_public_event(
                &node,
                "pod_created",
                &pod.slug,
                json!({"pod": pod, "package": package}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
        SensitiveChange::PublishPod { pod_id } => {
            let tenant_id = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?
                .tenant_id;
            store.assert_tenant(tenant_id, ctx.tenant_id)?;
            let pod = store
                .pods
                .get_mut(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            if pod.visibility == Visibility::Public {
                return Err(StoreError::Validation("Pod is already public".to_string()).into());
            }
            pod.visibility = Visibility::Public;
            let pod = pod.clone();
            if let Some(rules) = store.pod_rules.get_mut(pod_id) {
                rules.federate_sources = true;
            }
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let event = sign_public_event(
                &node,
                "pod_published",
                &pod.slug,
                json!({"pod": pod}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
        SensitiveChange::ExpandHarnessGrant {
            harness_id,
            capabilities,
            pod_ids,
        } => {
            let target = store
                .agent_harnesses
                .get_mut(harness_id)
                .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {harness_id}")))?;
            if target.tenant_id != ctx.tenant_id {
                return Err(StoreError::TenantBoundary.into());
            }
            let requested_capabilities = normalize_capabilities(capabilities.clone());
            if target
                .grant
                .capabilities
                .iter()
                .any(|capability| !requested_capabilities.contains(capability))
                || !grant_scope_expands(&target.grant.pod_ids, pod_ids)
            {
                return Err(StoreError::Validation(
                    "Harness Grant changed after proposal creation".to_string(),
                )
                .into());
            }
            target.grant.capabilities = requested_capabilities;
            target.grant.pod_ids = pod_ids.clone().map(normalize_pod_ids);
        }
        SensitiveChange::AddTrustedPeer {
            display_name,
            base_url,
            public_key,
        } => {
            if store
                .trusted_peers
                .values()
                .any(|peer| peer.tenant_id == ctx.tenant_id && peer.base_url == *base_url)
            {
                return Err(StoreError::Duplicate(format!("trusted peer {base_url}")).into());
            }
            let peer = TrustedPeer {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                display_name: display_name.clone(),
                base_url: base_url.clone(),
                public_key: public_key.clone(),
                trust_level: TrustLevel::ReadOnly,
                enabled: true,
                created_at: Utc::now(),
            };
            store.trusted_peers.insert(peer.id, peer);
        }
        SensitiveChange::RevisePublicPodPackage {
            pod_id,
            base_version,
            patch,
        } => {
            let pod = store
                .pods
                .get(pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
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
            let mut package = patch_skill_pack(existing, patch.clone());
            let validation = validate_skill_pack(&package);
            if !validation.valid {
                return Err(StoreError::Validation(validation.errors.join(", ")).into());
            }
            let now = Utc::now();
            package.created_at = now;
            package.updated_at = now;
            package.proposer_harness_id = Some(proposer);
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let event = sign_public_event(
                &node,
                "pod_skill_pack_updated",
                &pod.slug,
                json!({"package": package}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.insert_pod_package_version(package.clone())?;
            store.pod_skill_packs.insert(*pod_id, package);
            store.event_log.push(event);
        }
        SensitiveChange::RemovePublicSubmissionFromPod {
            pod_id,
            submission_id,
        } => {
            let pod = store
                .pods
                .get(pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            let before = store.submission_pods.len();
            store.submission_pods.retain(|placement| {
                !(placement.pod_id == *pod_id && placement.submission_id == *submission_id)
            });
            if store.submission_pods.len() == before {
                return Err(StoreError::Validation(
                    "public Pod Placement changed after proposal creation".to_string(),
                )
                .into());
            }
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let event = sign_public_event(
                &node,
                "link_removed",
                &pod.slug,
                json!({"submission_id": submission_id, "submission_purged": false}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
    }
    Ok(())
}

fn grant_scope_expands(current: &Option<Vec<PodId>>, requested: &Option<Vec<PodId>>) -> bool {
    match (current, requested) {
        (None, None) | (Some(_), None) => true,
        (Some(current), Some(requested)) => current.iter().all(|pod_id| requested.contains(pod_id)),
        (None, Some(_)) => false,
    }
}

fn ensure_child_pod_scope(
    parent: &Option<Vec<PodId>>,
    child: &Option<Vec<PodId>>,
) -> Result<(), AgentToolsError> {
    match (parent, child) {
        (Some(_), None) => Err(AgentToolsError::Forbidden {
            reason: "a harness cannot delegate a broader Pod scope".to_string(),
        }),
        (Some(parent), Some(child)) if child.iter().any(|pod_id| !parent.contains(pod_id)) => {
            Err(AgentToolsError::Forbidden {
                reason: "a harness cannot delegate a broader Pod scope".to_string(),
            })
        }
        _ => Ok(()),
    }
}

fn effective_user_id(ctx: &AuthContext, requested: Option<UserId>) -> Option<UserId> {
    if ctx.harness_id.is_some() {
        ctx.user_id
    } else {
        requested.or(ctx.user_id)
    }
}

fn normalize_pod_ids(mut pod_ids: Vec<PodId>) -> Vec<PodId> {
    pod_ids.sort();
    pod_ids.dedup();
    pod_ids
}

fn route_tokens(text: &str) -> Vec<String> {
    let stop = [
        "the",
        "and",
        "for",
        "with",
        "pod",
        "this",
        "that",
        "from",
        "into",
        "links",
        "link",
        "discovery",
        "personal",
        "public",
        "private",
        "use",
        "when",
        "brief",
        "style",
        "good",
        "bad",
        "stuff",
        "weird",
    ];
    let mut out = Vec::new();
    for token in text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() > 3)
    {
        if !stop.contains(&token) && !out.iter().any(|existing| existing == token) {
            out.push(token.to_string());
        }
        if out.len() >= 80 {
            break;
        }
    }
    out
}
