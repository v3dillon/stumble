use super::super::prelude::*;
use super::super::*;

impl AgentTools {
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

    /// Returns Pod workflow actions allowed by relationship, Harness Grant, and Pod scope.
    pub fn pod_allowed_actions(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<PodAllowedAction>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness = harness_for_context(&store, ctx)?;
        let capability = |capability| {
            harness.is_none_or(|harness| {
                harness.grant.capabilities.contains(&capability)
                    && harness
                        .grant
                        .pod_ids
                        .as_ref()
                        .is_none_or(|pod_ids| pod_ids.contains(&pod_id))
            })
        };
        let subscribed = ctx.user_id.is_some_and(|user_id| {
            store.subscriptions.values().any(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
        });
        let role = ctx.user_id.and_then(|user_id| {
            store
                .pod_roles
                .iter()
                .find(|assignment| assignment.user_id == user_id && assignment.pod_id == pod_id)
                .map(|assignment| assignment.role.clone())
        });
        let mut actions = Vec::new();
        if capability(HarnessCapability::SubscriptionManagement) {
            if subscribed {
                actions.extend([
                    PodAllowedAction::Unsubscribe,
                    PodAllowedAction::SubscriptionSet,
                ]);
            } else {
                actions.push(PodAllowedAction::Subscribe);
            }
        }
        if capability(HarnessCapability::PodCuration) && role.is_some() {
            actions.push(PodAllowedAction::RoleList);
            if role == Some(PodRole::Owner) {
                actions.extend([
                    PodAllowedAction::VisibilitySet,
                    PodAllowedAction::RoleGrant,
                    PodAllowedAction::RoleRevoke,
                ]);
            }
        }
        Ok(actions)
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

    /// Lists canonical Pod Roles for an authorized Owner or Curator.
    pub fn list_pod_roles(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<PodRoleAssignment>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod_id))?;
        let mut roles = store
            .pod_roles
            .iter()
            .filter(|assignment| assignment.pod_id == pod_id)
            .cloned()
            .collect::<Vec<_>>();
        roles.sort_by_key(|assignment| (assignment.created_at, assignment.user_id));
        Ok(roles)
    }

    /// Returns the Pod-owned Curation Policy, including its configured threshold.
    pub fn pod_curation_policy(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<CurationPolicy, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        Ok(store
            .pod_curation_policies
            .get(&pod_id)
            .copied()
            .unwrap_or_default())
    }

    /// Lists canonical Content Items with an Accepted Placement in one Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is missing, outside local curation scope,
    /// or the store lock is poisoned.
    pub fn list_content_items_for_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<ContentItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        let mut items = store
            .pod_placements
            .values()
            .filter(|placement| {
                placement.pod_id == pod_id && placement.status == PodPlacementStatus::Accepted
            })
            .filter_map(|placement| placement.content_item_id)
            .filter_map(|content_item_id| {
                store
                    .submissions
                    .get(&Uuid::from(content_item_id))
                    .map(ContentItem::from)
            })
            .collect::<Vec<_>>();
        items.sort_by_key(ContentItem::id);
        Ok(items)
    }

    /// Lists a Pod's complete accepted stream independently of Feed selection.
    ///
    /// This includes local and synchronized Accepted Placements visible through
    /// the caller's Feed-read grant and never applies ranking or delivery state.
    pub fn pod_content_stream(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<PodContentItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, Some(pod_id))?;
        let mut items = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.pod_id == pod_id)
            .map(|accepted_placement| {
                let submission = store
                    .submissions
                    .get(&Uuid::from(accepted_placement.content_item_id))
                    .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
                Ok(PodContentItem {
                    content_item: ContentItem::from(submission),
                    accepted_placement: accepted_placement.clone(),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        items.sort_by_key(|item| {
            (
                item.accepted_placement.accepted_at,
                item.accepted_placement.content_item_id,
            )
        });
        Ok(items)
    }

    /// Lists synchronization-safe Accepted Placement evidence for one visible Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the Pod is missing or outside
    /// the Harness Grant, the tenant boundary differs, or the lock is poisoned.
    pub fn accepted_placements_for_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<AcceptedPlacementProjection>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, Some(pod_id))?;
        let mut placements = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.pod_id == pod_id)
            .cloned()
            .collect::<Vec<_>>();
        placements.sort_by_key(|placement| (placement.accepted_at, placement.content_item_id));
        Ok(placements)
    }

    /// Reads one locally governed Pod Placement with retained origin provenance.
    ///
    /// # Errors
    ///
    /// Returns an error when local Pod curation is denied, the placement is
    /// missing, or the Home Node store lock is poisoned.
    pub fn pod_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        pod_id: PodId,
    ) -> Result<PodPlacement, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        store
            .pod_placements
            .get(&(candidate_id, pod_id))
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()).into())
    }

    /// Lists private Saves with any signed origin-withdrawal provenance.
    ///
    /// # Errors
    ///
    /// Returns an error when feedback access is denied, no User is authenticated,
    /// or the Home Node store lock is poisoned.
    pub fn saved_content_references(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<SavedContentReference>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Saved Content References require an authenticated User".into())
        })?;
        let mut saved = store
            .saves
            .iter()
            .filter(|(saved_user_id, _)| *saved_user_id == user_id)
            .filter_map(|(_, submission_id)| store.submissions.get(submission_id))
            .map(|item| {
                let content_item_id = ContentItemId::from(item.id);
                SavedContentReference {
                    content_reference: feed_content_reference(item),
                    origin_withdrawals: store
                        .placement_tombstones
                        .iter()
                        .filter(|tombstone| {
                            tombstone.origin_placement.content_item_id == content_item_id
                        })
                        .cloned()
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        saved.sort_by_key(|saved| saved.content_reference.content_item_id);
        Ok(saved)
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
}
