use super::super::prelude::*;
use super::super::*;

impl AgentTools {
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

    pub(crate) fn filter_brief_candidates(
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
}
