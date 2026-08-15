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
        let watches = list_watches(&store, user_id, ctx.tenant_id);
        let readiness = readiness(&store, user_id, ctx.tenant_id);
        let taste = taste_profile_from_store(&store, ctx, user_id)?;
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

    /// Removes one private User-scoped watch owned by the caller.
    pub fn remove_user_watch(
        &self,
        ctx: &AuthContext,
        watch_id: UserWatchId,
    ) -> Result<UserWatch, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("watches require an authenticated User".into())
        })?;
        let watch = store
            .user_watches
            .get(&watch_id)
            .ok_or_else(|| StoreError::NotFound("watch".into()))?
            .clone();
        if watch.user_id != user_id || watch.tenant_id != ctx.tenant_id {
            return Err(AgentToolsError::Forbidden {
                reason: "watch belongs to another User".into(),
            });
        }
        let mut staged = store.clone();
        staged.user_watches.remove(&watch_id);
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::RemoveUserWatch,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(watch)
    }

    /// Composes one morning brief from existing Home Node operations.
    ///
    /// Every section is present. Missing Feed or Explore access yields an empty
    /// network section and an inspectable gap instead of failing the brief.
    pub fn compose_brief(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<MorningBrief, AgentToolsError> {
        let packet = self.user_context_packet(ctx)?;
        let taste_summary = taste_summary(&packet.taste);

        let mut batches = self.list_discovery_result_batches(ctx)?;
        batches.retain(|batch| batch.state == DiscoveryResultBatchState::Ready);
        batches.sort_by_key(|batch| (batch.created_at, batch.id));
        let outside = match batches.pop() {
            Some(batch) => MorningBriefOutside {
                batch_id: Some(batch.id),
                items: batch.items,
                source_availability: batch.source_availability,
                reason: None,
            },
            None => MorningBriefOutside {
                batch_id: None,
                items: Vec::new(),
                source_availability: Vec::new(),
                reason: Some("no_unreviewed_batch".into()),
            },
        };

        let mut gaps: Vec<MorningBriefGap> = Vec::new();
        let feed = match FeedBatchRequest::new(7) {
            Ok(request) => match self.get_feed_batch(ctx, request, now) {
                Ok(batch) => batch.items,
                Err(AgentToolsError::Forbidden { .. }) => {
                    gaps.push(MorningBriefGap {
                        state: "feed_read_required".into(),
                        source: None,
                        watch_id: None,
                        url: None,
                        fingerprint: None,
                    });
                    Vec::new()
                }
                Err(error) => return Err(error),
            },
            Err(_) => Vec::new(),
        };

        let explore = match ExploreRequest::new("", 1, 3) {
            Ok(request) => match self.explore_public_pods(ctx, request) {
                Ok(response) => {
                    let mut results = response.results;
                    results.truncate(1);
                    results
                }
                Err(AgentToolsError::Forbidden { .. }) => {
                    if !gaps.iter().any(|gap| gap.state == "feed_read_required") {
                        gaps.push(MorningBriefGap {
                            state: "feed_read_required".into(),
                            source: None,
                            watch_id: None,
                            url: None,
                            fingerprint: None,
                        });
                    }
                    Vec::new()
                }
                Err(error) => return Err(error),
            },
            Err(_) => Vec::new(),
        };

        for watch in &packet.watches {
            if let Some(availability) = &watch.last_availability {
                if availability.state.authentication_required() {
                    gaps.push(MorningBriefGap {
                        state: availability.state.fingerprint_label().to_string(),
                        source: Some(availability.source.clone()),
                        watch_id: Some(watch.id),
                        url: Some(watch.url.clone()),
                        fingerprint: None,
                    });
                }
            }
        }
        for notice in self
            .list_authentication_needed_notices(ctx)?
            .into_iter()
            .filter(|notice| notice.delivery_pending)
        {
            if gaps
                .iter()
                .any(|gap| gap.source.as_deref() == Some(notice.source.as_str()))
            {
                continue;
            }
            gaps.push(MorningBriefGap {
                state: "authentication_required".into(),
                source: Some(notice.source),
                watch_id: None,
                url: None,
                fingerprint: Some(notice.state_fingerprint),
            });
        }
        if explore.is_empty() && !gaps.iter().any(|gap| gap.state == "feed_read_required") {
            gaps.push(MorningBriefGap {
                state: "no_announcements".into(),
                source: None,
                watch_id: None,
                url: None,
                fingerprint: None,
            });
        }

        Ok(MorningBrief {
            user: MorningBriefUser {
                context_md: packet.context_md,
                taste_summary,
            },
            outside,
            network: MorningBriefNetwork { feed, explore },
            gaps,
        })
    }
}

fn taste_summary(taste: &TasteProfile) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !taste.explicit.interests.is_empty() {
        parts.push(format!(
            "interests: {}",
            taste.explicit.interests.join(", ")
        ));
    }
    if !taste.explicit.blocked_topics.is_empty() {
        parts.push(format!(
            "blocked topics: {}",
            taste.explicit.blocked_topics.join(", ")
        ));
    }
    if !taste.explicit.blocked_sources.is_empty() {
        parts.push(format!(
            "blocked sources: {}",
            taste.explicit.blocked_sources.join(", ")
        ));
    }
    parts.join("; ")
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
