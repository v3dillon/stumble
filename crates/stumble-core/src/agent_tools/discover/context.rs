//! Private User Context and User-scoped watches.
//!
//! Interactive-only: the same unscoped interactive grant policy as Personal
//! Discovery management. Unattended personal_discovery_execution workers can
//! never read the User Context, the Taste Profile, or the watch list; they see
//! only the minimized Discovery Plan.

use super::super::prelude::*;
use super::super::*;

const MAX_CONTEXT_MD_LEN: usize = 65_536;
const MAX_WATCH_SKILL_LEN: usize = 120;

/// Hosts whose timeline and account watches default to the `watch-x` skill.
const WATCH_X_HOSTS: &[&str] = &["x.com", "twitter.com"];

impl AgentTools {
    /// Returns the one interactive briefing packet: context, taste, watches,
    /// readiness, and allowed actions.
    pub fn user_context_packet(
        &self,
        ctx: &AuthContext,
    ) -> Result<UserContextPacket, AgentToolsError> {
        let (context_md, watches, readiness) = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_personal_discovery_management(&store, ctx)?;
            let user_id = ctx.user_id.ok_or_else(|| {
                StoreError::Validation("User Context requires an authenticated User".into())
            })?;
            let context_md = store
                .user_contexts
                .get(&(user_id, ctx.tenant_id))
                .map(|context| context.context_md.clone())
                .unwrap_or_default();
            (
                context_md,
                list_watches(&store, user_id, ctx.tenant_id),
                readiness(&store, user_id, ctx.tenant_id),
            )
        };
        let taste = self.taste_profile(ctx)?;
        Ok(UserContextPacket {
            context_md,
            taste,
            watches,
            readiness,
            allowed_actions: vec![UserContextAllowedAction::Set],
        })
    }

    /// Replaces the private User Context prose (interactive User only).
    pub fn set_user_context(
        &self,
        ctx: &AuthContext,
        request: SetUserContextRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<UserContext, AgentToolsError> {
        if request.context_md.len() > MAX_CONTEXT_MD_LEN {
            return Err(StoreError::Validation(format!(
                "context_md must be at most {MAX_CONTEXT_MD_LEN} bytes"
            ))
            .into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("User Context requires an authenticated User".into())
        })?;
        let mut staged = store.clone();
        let context = staged
            .user_contexts
            .entry((user_id, ctx.tenant_id))
            .and_modify(|context| {
                context.context_md = request.context_md.clone();
                context.updated_at = now;
            })
            .or_insert_with(|| UserContext {
                user_id,
                tenant_id: ctx.tenant_id,
                context_md: request.context_md.clone(),
                created_at: now,
                updated_at: now,
            })
            .clone();
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::SetUserContext,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(context)
    }

    /// Adds a private User-scoped watch (interactive User only).
    pub fn add_user_watch(
        &self,
        ctx: &AuthContext,
        request: AddUserWatchRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<UserWatch, AgentToolsError> {
        let url = parse_public_url(&request.url, "watch url")?;
        validate_public_scheme_and_host(&url, "watch url")?;
        let host = url
            .domain()
            .map(str::to_lowercase)
            .ok_or_else(|| StoreError::Validation("watch url must have a domain".into()))?;
        let skill = match request.skill {
            Some(skill) => {
                let skill = skill.trim().to_string();
                if skill.is_empty() || skill.len() > MAX_WATCH_SKILL_LEN {
                    return Err(StoreError::Validation(format!(
                        "watch skill must be 1..={MAX_WATCH_SKILL_LEN} characters"
                    ))
                    .into());
                }
                Some(skill)
            }
            None => default_watch_skill(&host, request.kind),
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("watches require an authenticated User".into())
        })?;
        let normalized = normalized_url(url);
        if store.user_watches.values().any(|watch| {
            watch.user_id == user_id
                && watch.tenant_id == ctx.tenant_id
                && watch.url.eq_ignore_ascii_case(&normalized)
        }) {
            return Err(StoreError::Duplicate(format!("watch for {normalized}")).into());
        }
        let watch = UserWatch {
            id: Uuid::now_v7().into(),
            user_id,
            tenant_id: ctx.tenant_id,
            url: normalized,
            kind: request.kind,
            cadence: request.cadence.unwrap_or_default(),
            skill,
            last_availability: None,
            last_planned_at: None,
            created_at: now,
        };
        let mut staged = store.clone();
        staged.user_watches.insert(watch.id, watch.clone());
        record_harness_write(&mut staged, ctx, HarnessWriteOperation::AddUserWatch, None);
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(watch)
    }

    /// Lists the User's private watches in creation order.
    pub fn list_user_watches(&self, ctx: &AuthContext) -> Result<Vec<UserWatch>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("watches require an authenticated User".into())
        })?;
        Ok(list_watches(&store, user_id, ctx.tenant_id))
    }
}

fn list_watches(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
) -> Vec<UserWatch> {
    let mut watches: Vec<UserWatch> = store
        .user_watches
        .values()
        .filter(|watch| watch.user_id == user_id && watch.tenant_id == tenant_id)
        .cloned()
        .collect();
    watches.sort_by_key(|watch| (watch.created_at, watch.id));
    watches
}

/// The default skill for X timeline and account watches.
fn default_watch_skill(host: &str, kind: UserWatchKind) -> Option<String> {
    let x_host = WATCH_X_HOSTS
        .iter()
        .any(|candidate| host == *candidate || host.ends_with(&format!(".{candidate}")));
    (x_host && matches!(kind, UserWatchKind::Timeline | UserWatchKind::Account))
        .then(|| "watch-x".to_string())
}
