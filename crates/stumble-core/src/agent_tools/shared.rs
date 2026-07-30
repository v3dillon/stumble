use super::prelude::*;
use super::{
    apply_trust_policy_change, create_pod_lifecycle_locked, curation_actor, feed_content_reference,
    normalize_pod_ids, origin_placement_identity, pod_roles_value, visibility_exposure,
    AgentToolsError, PodCreationMode,
};

/// Cryptographic server entropy used for production peer-advertisement sampling.
pub(crate) fn server_sample_seed() -> u64 {
    OsRng.next_u64()
}

pub fn canonicalize_url(value: &str) -> Result<String, AgentToolsError> {
    canonicalize_url_spelling(value).map_err(|error| AgentToolsError::BadUrl(error.to_string()))
}

pub(crate) fn insert_normalized_policy_term(
    values: &mut std::collections::BTreeSet<String>,
    value: &str,
    field: &str,
) -> Result<(), AgentToolsError> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Err(StoreError::Validation(format!("{field} must not be empty")).into());
    }
    values.insert(value);
    Ok(())
}

pub(crate) fn parse_public_url(value: &str, field: &str) -> Result<Url, AgentToolsError> {
    let url = Url::parse(value)
        .map_err(|error| StoreError::Validation(format!("{field} is not a valid URL: {error}")))?;
    if url.username() != "" || url.password().is_some() {
        return Err(StoreError::Validation(format!("{field} must not include credentials")).into());
    }
    Ok(url)
}

pub(crate) fn validate_public_scheme_and_host(
    url: &Url,
    field: &str,
) -> Result<(), AgentToolsError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(StoreError::Validation(format!("{field} must use http or https")).into());
    }
    if url.host_str().is_none() {
        return Err(StoreError::Validation(format!("{field} must include a host")).into());
    }
    if !public_url_is_loopback(url) && url.scheme() != "https" {
        return Err(StoreError::Validation(format!(
            "{field} must use https unless it is loopback-only"
        ))
        .into());
    }
    Ok(())
}

pub(crate) fn public_url_is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

pub(crate) fn normalized_url(url: Url) -> String {
    url.to_string().trim_end_matches('/').to_string()
}

pub(crate) fn normalize_unique(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        let value = value.trim().to_lowercase();
        if !value.is_empty() && !output.contains(&value) {
            output.push(value);
        }
    }
    output
}

pub(crate) fn normalize_unique_case_insensitive(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.to_lowercase()))
        .collect()
}

pub(crate) fn normalize_source_affinity_signals(
    values: Vec<SourceAffinitySignal>,
) -> Vec<SourceAffinitySignal> {
    let mut output = Vec::new();
    for signal in values {
        let Some(normalized) = signal.normalized() else {
            continue;
        };
        if output
            .iter()
            .any(|existing: &SourceAffinitySignal| existing.eq_ignore_ascii_case(&normalized))
        {
            continue;
        }
        output.push(normalized);
    }
    output
}

pub(crate) fn authorize_harness(
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
    if capability == HarnessCapability::PodCuration {
        if let (Some(user_id), Some(pod_id)) = (ctx.user_id, pod_id) {
            if !store.pod_roles.iter().any(|assignment| {
                assignment.user_id == user_id
                    && assignment.pod_id == pod_id
                    && matches!(assignment.role, PodRole::Owner | PodRole::Curator)
            }) {
                return Err(AgentToolsError::Forbidden {
                    reason: format!("User has no Pod Role for Pod {pod_id}"),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn agent_harness_view(
    store: &InMemoryStore,
    harness: &AgentHarness,
) -> Result<AgentHarnessView, AgentToolsError> {
    let token_hash = store
        .api_tokens
        .values()
        .find(|token| token.harness_id == Some(harness.id))
        .map(|token| token.token_hash.as_str())
        .ok_or_else(|| {
            StoreError::NotFound(format!("credential for Agent Harness {}", harness.id))
        })?;
    let prefix = &token_hash[..token_hash.len().min(12)];
    Ok(AgentHarnessView {
        harness: harness.clone(),
        credential_fingerprint: format!("sha256:{prefix}"),
        status: if harness.revoked_at.is_some() {
            AgentHarnessStatus::Revoked
        } else {
            AgentHarnessStatus::Active
        },
    })
}

/// Stable local Home Node owner User: earliest `created_at`, then lowest id.
///
/// Used by owner credential auth and by harness registration when the caller
/// has no User, so Trust Policy and Personal Discovery share one User key.
pub(crate) fn local_owner_user_id(store: &InMemoryStore) -> Option<UserId> {
    store
        .users
        .values()
        .min_by_key(|user| (user.created_at, user.id))
        .map(|user| user.id)
}

pub(crate) fn harness_for_context<'a>(
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

pub(crate) fn record_harness_write(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    operation: HarnessWriteOperation,
    pod_id: Option<PodId>,
) {
    record_harness_write_at(store, ctx, operation, pod_id, Utc::now());
}

pub(crate) fn record_harness_write_at(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    operation: HarnessWriteOperation,
    pod_id: Option<PodId>,
    occurred_at: chrono::DateTime<Utc>,
) {
    if let Some(harness_id) = ctx.harness_id {
        store.harness_write_audit.push(HarnessWriteAudit {
            id: Uuid::now_v7(),
            harness_id,
            operation,
            pod_id,
            occurred_at,
        });
    }
}

pub(crate) fn extend_unique<T: PartialEq>(
    retained: &mut Vec<T>,
    additional: impl IntoIterator<Item = T>,
) {
    for value in additional {
        if !retained.contains(&value) {
            retained.push(value);
        }
    }
}

pub(crate) fn normalize_capabilities(
    mut capabilities: Vec<HarnessCapability>,
) -> Vec<HarnessCapability> {
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

pub(crate) fn authorize_proposal_reader(
    store: &InMemoryStore,
    ctx: &AuthContext,
    proposal_id: PendingProposalId,
) -> Result<(), AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    let Some(harness) = harness_for_context(store, ctx)? else {
        if ctx.tenant_id == proposal.tenant_id
            && (ctx.user_id.is_none() || ctx.user_id == Some(proposal.user_id))
        {
            return Ok(());
        }
        return Err(AgentToolsError::Forbidden {
            reason: "Pending Proposal belongs to another User or tenant".to_string(),
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

pub(crate) fn authorize_independent_approver(
    store: &InMemoryStore,
    ctx: &AuthContext,
    proposal_id: PendingProposalId,
) -> Result<ProposalDecisionActor, AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    let Some(harness) = harness_for_context(store, ctx)? else {
        if ctx.tenant_id == proposal.tenant_id && ctx.user_id == Some(proposal.user_id) {
            return Ok(ProposalDecisionActor::Owner {
                owner_user_id: proposal.user_id,
            });
        }
        return Err(AgentToolsError::Forbidden {
            reason: "approval must belong to the proposal User and tenant".to_string(),
        });
    };
    authorize_harness(store, ctx, HarnessCapability::Approval, None)?;
    if harness.kind != AgentHarnessKind::Interactive {
        return Err(AgentToolsError::Forbidden {
            reason: "approval requires an interactive Agent Harness".to_string(),
        });
    }
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
    Ok(ProposalDecisionActor::Harness(harness.id))
}

pub(crate) fn approval_scope_allows(harness: &AgentHarness, proposal: &PendingProposal) -> bool {
    proposal
        .affected_resources
        .iter()
        .all(|resource| match resource {
            ProposalResource::Pod(pod_id)
            | ProposalResource::PodPackage(pod_id)
            | ProposalResource::PodCurationPolicy(pod_id)
            | ProposalResource::PodRoles(pod_id)
            | ProposalResource::SubmissionPlacement { pod_id, .. } => harness
                .grant
                .pod_ids
                .as_ref()
                .is_none_or(|pod_ids| pod_ids.contains(pod_id)),
            ProposalResource::PodSlug(_)
            | ProposalResource::AgentHarness(_)
            | ProposalResource::TrustedPeerUrl(_)
            | ProposalResource::TrustPolicy(_) => harness.grant.pod_ids.is_none(),
        })
}

pub(crate) fn expire_proposal(
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

pub(crate) fn validate_structured_diff(
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
            ProposalResource::TrustPolicy(user_id) => json!(store
                .trust_policies
                .get(&(*user_id, proposal.tenant_id))
                .cloned()
                .unwrap_or_else(|| TrustPolicy::new(*user_id, proposal.tenant_id))),
            ProposalResource::PodPackage(pod_id) => store
                .pod_skill_packs
                .get(pod_id)
                .map_or(serde_json::Value::Null, |package| json!(package)),
            ProposalResource::PodCurationPolicy(pod_id) => json!({
                "curation_policy": store
                    .pod_curation_policies
                    .get(pod_id)
                    .copied()
                    .unwrap_or_default()
            }),
            ProposalResource::PodRoles(pod_id) => pod_roles_value(store, *pod_id),
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

/// Expands a Pod's visibility, signing the `pod_published` federation event
/// when it becomes public. Shared by proposal approval and direct Owner action.
pub(crate) fn apply_expand_pod_visibility(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    pod_id: &PodId,
    visibility: &Visibility,
) -> Result<(), AgentToolsError> {
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
    if visibility_exposure(visibility) <= visibility_exposure(&pod.visibility) {
        return Err(
            StoreError::Validation("approved visibility must expand exposure".into()).into(),
        );
    }
    pod.visibility = visibility.clone();
    let pod = pod.clone();
    if let Some(rules) = store.pod_rules.get_mut(pod_id) {
        rules.federate_sources = *visibility == Visibility::Public;
    }
    if *visibility == Visibility::Public {
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let package = store
            .pod_skill_packs
            .get(pod_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Pod Package".to_string()))?;
        let event = sign_public_event(
            &node,
            "pod_published",
            &pod.slug,
            json!({"pod": pod, "package": package}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        // Federation serves history from this publication onward, so re-emit
        // the accepted content that should travel with the now-public Pod.
        // Anything placed (or removed) while private stays local.
        let mut placements: Vec<AcceptedPlacementProjection> = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.pod_id == *pod_id)
            .cloned()
            .collect();
        placements.sort_by_key(|placement| (placement.accepted_at, placement.content_item_id));
        for placement in placements {
            let item = store
                .submissions
                .get(&Uuid::from(placement.content_item_id))
                .map(ContentItem::from)
                .ok_or_else(|| StoreError::NotFound("Content Item".to_string()))?;
            let event = sign_public_event(
                &node,
                "content_item_placed",
                &pod.slug,
                json!({
                    "content_item": item,
                    "accepted_placement": placement,
                }),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
    }
    Ok(())
}

pub(crate) fn apply_sensitive_change(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    proposer: AgentHarnessId,
    requested_change: &SensitiveChange,
) -> Result<(), AgentToolsError> {
    match requested_change {
        SensitiveChange::CreatePublicPod { request } => {
            create_pod_lifecycle_locked(
                store,
                ctx,
                CreatePodLifecycleRequest {
                    pod: request.clone(),
                    package: PodCreationPackage::Default,
                },
                Some(proposer),
                PodCreationMode::LegacyPublic,
            )?;
        }
        SensitiveChange::CreatePublicPodLifecycle { request } => {
            create_pod_lifecycle_locked(
                store,
                ctx,
                request.clone(),
                Some(proposer),
                PodCreationMode::Canonical,
            )?;
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
            let package = store
                .pod_skill_packs
                .get(pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Pod Package".to_string()))?;
            let event = sign_public_event(
                &node,
                "pod_published",
                &pod.slug,
                json!({"pod": pod, "package": package}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
        SensitiveChange::ExpandPodVisibility { pod_id, visibility } => {
            apply_expand_pod_visibility(store, ctx, pod_id, visibility)?;
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
            node_id,
            display_name,
            base_url,
            public_key,
        } => {
            if store.trusted_peers.values().any(|peer| {
                peer.tenant_id == ctx.tenant_id
                    && (peer.base_url == *base_url
                        || (!node_id.is_nil() && peer.node_id == *node_id))
            }) {
                return Err(StoreError::Duplicate(format!("trusted peer {base_url}")).into());
            }
            let peer = TrustedPeer {
                id: Uuid::now_v7(),
                node_id: *node_id,
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
        SensitiveChange::RemoveTrustedPeer { peer_id } => {
            let peer = store
                .trusted_peers
                .get_mut(peer_id)
                .ok_or_else(|| StoreError::NotFound(format!("trusted peer {peer_id}")))?;
            if peer.tenant_id != ctx.tenant_id {
                return Err(StoreError::TenantBoundary.into());
            }
            if !peer.enabled {
                return Err(
                    StoreError::Validation("trusted peer is already disabled".into()).into(),
                );
            }
            peer.enabled = false;
        }
        SensitiveChange::ChangeTrustPolicy { change } => {
            let user_id = store
                .agent_harnesses
                .get(&proposer)
                .map(|harness| harness.user_id)
                .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {proposer}")))?;
            let key = (user_id, ctx.tenant_id);
            let mut policy = store
                .trust_policies
                .get(&key)
                .cloned()
                .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
            apply_trust_policy_change(&mut policy, change)?;
            store.trust_policies.insert(key, policy);
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
            refresh_public_pod_announcement_if_needed(store, *pod_id, now)?;
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
            let content_item_id = ContentItemId::from(*submission_id);
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let origin_placement = store
                .accepted_placement_projections
                .get(&(content_item_id, *pod_id))
                .cloned();
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
            let withdrawn_at = Utc::now();
            if let Some(placement) = store.pod_placements.values_mut().find(|placement| {
                placement.pod_id == *pod_id
                    && placement.content_item_id == Some((*submission_id).into())
                    && placement.status == PodPlacementStatus::Accepted
            }) {
                let actor = curation_actor(ctx);
                placement.status = PodPlacementStatus::Reversed;
                placement.curation_path = CurationPath::ManualReview;
                placement.actor = actor;
                placement.updated_at = withdrawn_at;
                placement.audit_history.push(PlacementAuditEntry {
                    status: PodPlacementStatus::Reversed,
                    curation_path: CurationPath::ManualReview,
                    actor,
                    note: Some(CurationRationale::new(
                        "approved public placement reversal",
                    )?),
                    occurred_at: withdrawn_at,
                });
            }
            let event = if let Some(origin_placement) = origin_placement {
                let content_reference = store
                    .submissions
                    .get(submission_id)
                    .map(feed_content_reference)
                    .ok_or_else(|| StoreError::NotFound("Content Reference".into()))?;
                let tombstone = PlacementTombstone {
                    content_reference,
                    origin_placement,
                    withdrawn_at,
                };
                let tombstoned_origin_id = origin_placement_identity(&tombstone.origin_placement);
                for placement in store.pod_placements.values_mut().filter(|placement| {
                    placement.content_item_id == Some(content_item_id)
                        && placement
                            .origin_placements
                            .iter()
                            .map(origin_placement_identity)
                            .collect::<HashSet<_>>()
                            .contains(&tombstoned_origin_id)
                }) {
                    placement.origin_withdrawals.push(tombstone.clone());
                }
                store
                    .accepted_placement_projections
                    .remove(&(content_item_id, *pod_id));
                store.placement_tombstones.push(tombstone.clone());
                sign_public_event(
                    &node,
                    FederatedPodEventType::PlacementTombstoned.as_wire(),
                    &pod.slug,
                    json!({"placement_tombstone": tombstone}),
                    store.latest_event_hash(&pod.slug),
                )?
            } else {
                sign_public_event(
                    &node,
                    FederatedPodEventType::LegacyLinkRemoved.as_wire(),
                    &pod.slug,
                    json!({"submission_id": submission_id, "submission_purged": false}),
                    store.latest_event_hash(&pod.slug),
                )?
            };
            store.event_log.push(event);
            refresh_public_pod_announcement_if_needed(store, *pod_id, withdrawn_at)?;
        }
        SensitiveChange::EnableAutonomousCuration {
            pod_id,
            confidence_threshold,
        } => {
            let pod = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            store.pod_curation_policies.insert(
                *pod_id,
                CurationPolicy::Autonomous {
                    confidence_threshold: *confidence_threshold,
                },
            );
        }
        SensitiveChange::GrantPodRole {
            pod_id,
            user_id,
            role,
        } => {
            store.pod_roles.retain(|assignment| {
                assignment.pod_id != *pod_id || assignment.user_id != *user_id
            });
            store.pod_roles.push(PodRoleAssignment {
                user_id: *user_id,
                pod_id: *pod_id,
                role: role.clone(),
                created_at: Utc::now(),
            });
        }
        SensitiveChange::RevokePodRole {
            pod_id,
            user_id,
            role,
        } => {
            let before = store.pod_roles.len();
            store.pod_roles.retain(|assignment| {
                assignment.pod_id != *pod_id
                    || assignment.user_id != *user_id
                    || assignment.role != *role
            });
            if store.pod_roles.len() == before {
                return Err(StoreError::NotFound(format!("Pod Role for User {user_id}")).into());
            }
        }
    }
    Ok(())
}

pub(crate) fn grant_scope_expands(
    current: &Option<Vec<PodId>>,
    requested: &Option<Vec<PodId>>,
) -> bool {
    match (current, requested) {
        (None, None) | (Some(_), None) => true,
        (Some(current), Some(requested)) => current.iter().all(|pod_id| requested.contains(pod_id)),
        (None, Some(_)) => false,
    }
}

pub(crate) fn effective_user_id(ctx: &AuthContext, requested: Option<UserId>) -> Option<UserId> {
    if ctx.harness_id.is_some() {
        ctx.user_id
    } else {
        requested.or(ctx.user_id)
    }
}
