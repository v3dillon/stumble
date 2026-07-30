use super::super::prelude::*;
use super::super::*;

pub(crate) fn task_with_expired_lease_recorded(
    mut task: DiscoveryTask,
    now: chrono::DateTime<Utc>,
) -> DiscoveryTask {
    record_expired_lease(&mut task, now);
    task
}

pub(crate) fn record_expired_lease(task: &mut DiscoveryTask, now: chrono::DateTime<Utc>) {
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

pub(crate) fn agent_evidence_error_to_tools(error: AgentEvidenceError) -> AgentToolsError {
    match error {
        AgentEvidenceError::CapabilityDenied => AgentToolsError::Forbidden {
            reason: error.to_string(),
        },
        other => StoreError::Validation(other.to_string()).into(),
    }
}

pub(crate) fn authorized_discovery_task_mutation(
    store: &InMemoryStore,
    ctx: &AuthContext,
    task_id: DiscoveryTaskId,
) -> Result<(Option<PodId>, AgentHarnessId), AgentToolsError> {
    let task = store
        .discovery_tasks
        .get(&task_id)
        .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
    let pod_id = authorize_discovery_task(store, ctx, task)?;
    let harness_id = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
        reason: "task mutation requires an Agent Harness".into(),
    })?;
    Ok((pod_id, harness_id))
}

pub(crate) fn authorize_discovery_task(
    store: &InMemoryStore,
    ctx: &AuthContext,
    task: &DiscoveryTask,
) -> Result<Option<PodId>, AgentToolsError> {
    match task.target {
        DiscoveryTaskTarget::Pod { pod_id, .. } => {
            authorize_harness(store, ctx, HarnessCapability::DiscoveryTasks, Some(pod_id))?;
            Ok(Some(pod_id))
        }
        DiscoveryTaskTarget::Personal { .. } => {
            authorize_personal_discovery_execution(store, ctx)?;
            let plan_id = task
                .target
                .discovery_plan_id()
                .expect("Personal target carries a Discovery Plan ID");
            let plan = store
                .discovery_plans
                .get(&plan_id)
                .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;
            if ctx.user_id != Some(plan.user_id) || ctx.tenant_id != plan.tenant_id {
                return Err(AgentToolsError::Forbidden {
                    reason: "Personal Discovery task belongs to another User".into(),
                });
            }
            Ok(None)
        }
    }
}

pub(crate) fn authorize_personal_discovery_management(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    authorize_harness(
        store,
        ctx,
        HarnessCapability::PersonalDiscoveryManagement,
        None,
    )?;
    if let Some(harness) = harness_for_context(store, ctx)? {
        if harness.kind != AgentHarnessKind::Interactive || harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery management requires an unscoped interactive grant"
                    .into(),
            });
        }
    }
    Ok(())
}

pub(crate) fn authorize_personal_discovery_execution(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    authorize_harness(
        store,
        ctx,
        HarnessCapability::PersonalDiscoveryExecution,
        None,
    )?;
    if let Some(harness) = harness_for_context(store, ctx)? {
        if harness.kind != AgentHarnessKind::Unattended || harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery execution requires an unscoped unattended grant".into(),
            });
        }
    }
    Ok(())
}

/// Management or execution may inspect schedules and backpressure; only management may mutate.
pub(crate) fn authorize_personal_discovery_schedule_read(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    if authorize_personal_discovery_management(store, ctx).is_ok() {
        return Ok(());
    }
    authorize_personal_discovery_execution(store, ctx)
}

pub(crate) fn accept_discovery_result_into_pod(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
    submission: &CandidateSubmission,
    pod_id: PodId,
    reason: CurationRationale,
    now: chrono::DateTime<Utc>,
) -> Result<PodPlacement, AgentToolsError> {
    if let Some(existing) = store.pod_placements.get(&(candidate.id, pod_id)).cloned() {
        if existing.status == PodPlacementStatus::Accepted {
            return Ok(existing);
        }
    }
    let content_item = ensure_content_item(
        store,
        candidate,
        std::slice::from_ref(submission),
        &[submission.id],
        now,
    )?;
    let actor = curation_actor(ctx);
    let placement = PodPlacement {
        candidate_id: candidate.id,
        pod_id,
        content_item_id: Some(content_item.id()),
        reason,
        confidence: CandidateConfidence::new(1.0)
            .map_err(|error| StoreError::Validation(error.to_string()))?,
        source_submission_ids: vec![submission.id],
        origin_placements: Vec::new(),
        origin_withdrawals: Vec::new(),
        status: PodPlacementStatus::Accepted,
        curation_path: CurationPath::AddToPod,
        actor,
        audit_history: vec![PlacementAuditEntry {
            status: PodPlacementStatus::Accepted,
            curation_path: CurationPath::AddToPod,
            actor,
            note: None,
            occurred_at: now,
        }],
        created_at: now,
        updated_at: now,
    };
    accept_candidate_placement(store, ctx, candidate, &placement)?;
    enrich_accepted_content_item(store, ctx, candidate)?;
    store
        .pod_placements
        .insert((candidate.id, pod_id), placement.clone());
    Ok(placement)
}

/// Builds shared private LocalSimilarityContext for Explore and Feed ranking.
pub(crate) fn local_similarity_context_from_store(
    query: Option<&str>,
    preferences: Option<&UserPreferences>,
    projections: &TasteProfileProjections,
) -> LocalSimilarityContext {
    let interests = preferences
        .map(|prefs| {
            prefs
                .interests
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let source_signals = projections
        .source_affinities
        .iter()
        .filter(|affinity| affinity.weight > 0.0 && !affinity.explicitly_blocked)
        .filter_map(|affinity| match &affinity.signal {
            SourceAffinitySignal::Source(source) | SourceAffinitySignal::Community(source) => {
                Some(source.as_str())
            }
            _ => None,
        });
    LocalSimilarityContext::from_private_evidence(query, interests, source_signals)
}

/// Retained Origin-signed samples when verified; otherwise local content without
/// claiming verification (trial requires real retained Origin-signed samples).
pub(crate) fn retained_or_local_explore_samples(
    store: &InMemoryStore,
    announcement: &PodAnnouncement,
    policy: &TrustPolicy,
    tenant_id: Option<TenantId>,
    local_node_id: NodeIdentityId,
    sample_size: usize,
) -> (Vec<FeedContentReference>, bool) {
    if let Some(sample_set) = store.pod_explore_sample_sets.get(&announcement.id) {
        let verified = verify_explore_samples_for_announcement(sample_set, announcement).is_ok();
        let samples = sample_set
            .samples
            .iter()
            .filter(|sample| !policy.blocks_content_reference(sample))
            .take(sample_size)
            .cloned()
            .collect::<Vec<_>>();
        return (samples, verified);
    }
    let local_samples = explore_content_samples(
        store,
        tenant_id,
        local_node_id,
        announcement,
        policy,
        sample_size,
    );
    // Local content may inform scoring but never claims Origin-signed verification.
    (local_samples, false)
}

/// Whether retained Origin-signed explore samples verify for this announcement.
pub(crate) fn retained_samples_verified(store: &InMemoryStore, announcement: &PodAnnouncement) -> bool {
    store
        .pod_explore_sample_sets
        .get(&announcement.id)
        .is_some_and(|sample_set| {
            verify_explore_samples_for_announcement(sample_set, announcement).is_ok()
        })
}

/// Local similarity for an unsubscribed public Pod Exploration Item.
///
/// Uses the same eligibility and endorsement policy gates as Explore. Requires a
/// real current verified announcement — never fabricates synthetic announcements.
/// `samples_verified` only from retained Origin-signed samples.
pub(crate) fn exploration_similarity_for_item(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    item: &Submission,
    placement_pod_ids: &[PodId],
    preferences: Option<&UserPreferences>,
) -> Option<PodSimilarityScore> {
    let projections = taste_profile_projections(store, user_id, tenant_id, preferences);
    let local = local_similarity_context_from_store(None, preferences, &projections);
    if local.is_empty() {
        return None;
    }
    let policy = store
        .trust_policies
        .get(&(user_id, tenant_id))
        .cloned()
        .unwrap_or_else(|| TrustPolicy::new(user_id, tenant_id));
    let now = Utc::now();
    let sample_ref = feed_content_reference(item);
    let mut best: Option<PodSimilarityScore> = None;
    for pod_id in placement_pod_ids {
        let Some(pod) = store.pods.get(pod_id) else {
            continue;
        };
        let Some(origin) = pod.origin_node_id.or_else(|| {
            store
                .node_identities
                .values()
                .find(|node| node.tenant_id == tenant_id)
                .map(|node| node.id)
        }) else {
            continue;
        };
        // Skip when no verified current announcement — never fabricate Uuid::nil shells.
        let Some(known) = store
            .known_pod_announcements
            .get(&(origin, pod.slug.clone()))
        else {
            continue;
        };
        if !announcement_scoring_eligible(store, known, &policy, now) {
            continue;
        }
        let announcement = &known.announcement;
        let endorsements = collect_policy_endorsements(store, announcement, &policy);
        let context_text = store
            .pod_skill_packs
            .get(pod_id)
            .map(|package| package.context_md.as_str());
        let samples_verified = retained_samples_verified(store, announcement);
        let Some(similarity) = score_exploration_item(
            &local,
            announcement,
            context_text,
            &sample_ref,
            &endorsements,
            samples_verified,
        ) else {
            continue;
        };
        best = Some(match best {
            Some(existing) if existing.score >= similarity.score => existing,
            _ => similarity,
        });
    }
    best
}

/// Enforces per-Origin exploration and trial caps after Feed Mix selection.
pub(crate) fn apply_exploration_origin_caps<'a>(
    store: &InMemoryStore,
    _user_id: UserId,
    selected: Vec<RankedFeedCandidate<'a>>,
) -> Vec<RankedFeedCandidate<'a>> {
    let caps = ExplorationCaps {
        per_origin: MAX_RESULTS_PER_ORIGIN,
        per_pod: usize::MAX,
        per_source: usize::MAX,
        per_origin_trial: MAX_TRIAL_ITEMS_PER_ORIGIN,
    };
    let local_node_id = store.node_identities.values().next().map(|node| node.id);
    let mut tracker = ExplorationCapTracker::new();
    let mut kept = Vec::with_capacity(selected.len());
    for candidate in selected {
        if candidate.kind != FeedItemKind::Exploration {
            kept.push(candidate);
            continue;
        }
        let origin = candidate
            .pod_ids
            .iter()
            .find_map(|pod_id| {
                store
                    .pods
                    .get(pod_id)
                    .map(|pod| pod.origin_node_id.or(local_node_id))
            })
            .flatten()
            .or(local_node_id);
        let Some(origin) = origin else {
            kept.push(candidate);
            continue;
        };
        // Typed flag only — never infer trial from reason-string contains.
        let trial = candidate.trial_exposure;
        let pod_slug = candidate
            .pod_ids
            .first()
            .and_then(|pod_id| store.pods.get(pod_id).map(|pod| pod.slug.clone()))
            .unwrap_or_default();
        if !tracker.can_admit_origin(origin, caps) {
            continue;
        }
        if trial && !tracker.can_admit_trial(origin, caps) {
            continue;
        }
        tracker.record(origin, &pod_slug, Some(&candidate.item.domain), trial);
        kept.push(candidate);
    }
    kept
}

pub(crate) fn explore_content_samples(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    local_node_id: NodeIdentityId,
    announcement: &PodAnnouncement,
    policy: &TrustPolicy,
    sample_size: usize,
) -> Vec<FeedContentReference> {
    let Some(pod) = store.pods.values().find(|pod| {
        pod.tenant_id == tenant_id
            && pod.visibility == Visibility::Public
            && pod.slug == announcement.pod_slug
            && pod.origin_node_id.unwrap_or(local_node_id) == announcement.origin_node_id
    }) else {
        return Vec::new();
    };
    let mut samples = store
        .submissions
        .values()
        .filter(|item| item.tenant_id == tenant_id)
        .filter(|item| {
            store
                .accepted_placement_projections
                .contains_key(&(ContentItemId::from(item.id), pod.id))
        })
        .filter(|item| {
            !policy.blocks_source_and_topics(
                &item.domain,
                &item.tags,
                &item.title,
                item.summary.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    samples.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.canonical_url.cmp(&right.canonical_url))
    });
    samples
        .into_iter()
        .take(sample_size)
        .map(feed_content_reference)
        .collect()
}

