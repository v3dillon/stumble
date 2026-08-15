use super::super::prelude::*;
use super::super::*;
use crate::store::read_store_generation;

impl AgentTools {
    pub fn new(store: InMemoryStore) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            persistence: None,
            bootstrap: BootstrapCapability::default(),
            index: IndexCapability::default(),
            relay: RelayCapability::default(),
            discovery_peer_probe: Arc::new(UnreachableDiscoveryPeerProbe),
        }
    }

    pub fn new_sqlite_persistent(store: InMemoryStore, path: impl Into<PathBuf>) -> Self {
        let path: PathBuf = path.into();
        let generation = read_store_generation(&path).unwrap_or(0);
        let baseline = store_records(&store).unwrap_or_default();
        Self {
            store: Arc::new(RwLock::new(store)),
            persistence: Some(Persistence::Sqlite {
                path: Arc::new(path),
                baseline: Arc::new(Mutex::new(baseline)),
                generation: Arc::new(std::sync::atomic::AtomicI64::new(generation)),
            }),
            bootstrap: BootstrapCapability::default(),
            index: IndexCapability::default(),
            relay: RelayCapability::default(),
            discovery_peer_probe: Arc::new(UnreachableDiscoveryPeerProbe),
        }
    }

    pub fn open_home_node(
        data_dir: impl AsRef<Path>,
        seed: impl FnOnce() -> InMemoryStore,
    ) -> Result<Self, AgentToolsError> {
        let database_path = data_dir.as_ref().join("stumble.sqlite3");
        let store = load_or_initialize_sqlite_store(&database_path, seed)?;
        Ok(Self::new_sqlite_persistent(store, database_path))
    }

    /// Returns whether the selected path contains initialized Home Node state.
    pub fn home_node_is_initialized(data_dir: impl AsRef<Path>) -> Result<bool, AgentToolsError> {
        Ok(sqlite_home_node_is_initialized(
            &data_dir.as_ref().join("stumble.sqlite3"),
        )?)
    }

    /// Initializes a new Home Node and refuses to reopen an existing one.
    pub fn initialize_home_node(
        data_dir: impl AsRef<Path>,
        seed: impl FnOnce() -> InMemoryStore,
    ) -> Result<Self, AgentToolsError> {
        let data_dir = data_dir.as_ref();
        if Self::home_node_is_initialized(data_dir)? {
            return Err(AgentToolsError::NodeAlreadyInitialized);
        }
        Self::open_home_node(data_dir, seed)
    }

    /// Opens existing Home Node state without initializing an empty path.
    pub fn open_initialized_home_node(data_dir: impl AsRef<Path>) -> Result<Self, AgentToolsError> {
        let data_dir = data_dir.as_ref();
        let database_path = data_dir.join("stumble.sqlite3");
        if !sqlite_home_node_is_initialized(&database_path)? {
            return Err(AgentToolsError::NodeNotInitialized);
        }
        let store = load_sqlite_store(&database_path)?;
        Ok(Self::new_sqlite_persistent(store, database_path))
    }

    pub fn store(&self) -> Arc<RwLock<InMemoryStore>> {
        self.store.clone()
    }

    pub fn persistence_path(&self) -> Option<&Path> {
        match &self.persistence {
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

    /// Returns the automatically authenticated local User context for the Home
    /// Node Owner Credential.
    pub fn local_owner_auth_context(&self) -> Result<AuthContext, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.default_node()?;
        Ok(AuthContext {
            user_id: local_owner_user_id(&store),
            tenant_id: node.tenant_id,
            node_id: node.id,
            harness_id: None,
        })
    }

    pub(crate) fn persist_locked(&self, store: &mut InMemoryStore) -> Result<(), AgentToolsError> {
        match &self.persistence {
            Some(Persistence::Sqlite {
                path,
                baseline,
                generation,
            }) => {
                let mut baseline = baseline.lock().map_err(|_| AgentToolsError::LockPoisoned)?;
                match persist_sqlite_store_changes(path, &baseline, store) {
                    Ok((new_generation, current_records)) => {
                        generation.store(new_generation, std::sync::atomic::Ordering::SeqCst);
                        *baseline = current_records;
                    }
                    Err(error) => {
                        // Revert to the authoritative on-disk state; if even
                        // that fails, rebuild the store from the baseline
                        // records so the failed mutation never survives in
                        // memory.
                        if let Ok(authoritative) =
                            load_sqlite_store(path).or_else(|_| store_from_records(&baseline))
                        {
                            generation.store(
                                read_store_generation(path).unwrap_or(0),
                                std::sync::atomic::Ordering::SeqCst,
                            );
                            *baseline = store_records(&authoritative).unwrap_or_default();
                            *store = authoritative;
                        }
                        return Err(error.into());
                    }
                }
            }
            None => {}
        }
        Ok(())
    }

    /// Reloads in-memory state when another process has persisted newer store
    /// generations — the freshness guarantee for long-lived daemons whose
    /// SQLite store is also written by one-shot CLI commands.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be read or a lock is poisoned.
    pub fn refresh_if_stale(&self) -> Result<bool, AgentToolsError> {
        let Some(Persistence::Sqlite {
            path,
            baseline,
            generation,
        }) = &self.persistence
        else {
            return Ok(false);
        };
        let disk_generation = read_store_generation(path)?;
        if disk_generation == generation.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(false);
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let mut baseline = baseline.lock().map_err(|_| AgentToolsError::LockPoisoned)?;
        let authoritative = load_sqlite_store(path)?;
        *baseline = store_records(&authoritative)?;
        *store = authoritative;
        generation.store(disk_generation, std::sync::atomic::Ordering::SeqCst);
        Ok(true)
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

    pub(crate) fn create_tenant_inner(
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

    pub(crate) fn create_dev_token_inner(
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
            supported_protocol_version: CURRENT_PROTOCOL_VERSION.to_string(),
        })
    }
}
