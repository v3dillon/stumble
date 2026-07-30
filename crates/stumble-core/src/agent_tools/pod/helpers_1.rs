use super::super::prelude::*;
use super::super::*;

pub(crate) fn canonicalize_candidate_evidence_url(value: &str) -> Result<String, AgentToolsError> {
    let parsed = Url::parse(value).map_err(|error| AgentToolsError::BadUrl(error.to_string()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AgentToolsError::BadUrl(
            "Candidate evidence URLs must not contain credentials".into(),
        ));
    }
    canonicalize_url(value)
}

pub(crate) fn ensure_projected_pod_support(store: &mut InMemoryStore, pod: &Pod) {
    store.pod_rules.entry(pod.id).or_insert(PodRules {
        pod_id: pod.id,
        blocked_topics: vec![],
        blocked_domains: vec![],
        auto_promote_crawler_candidates: false,
        federate_sources: true,
    });
}

pub(crate) fn route_text(request: &RouteLinkRequest) -> String {
    format!(
        "{} {} {} {}",
        request.url,
        request.title.clone().unwrap_or_default(),
        request.summary.clone().unwrap_or_default(),
        request.tags.join(" ")
    )
    .to_lowercase()
}

pub(crate) fn suggest_new_pod_for_link(
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

pub(crate) fn unique_slug(base: String, existing_slugs: &HashSet<String>) -> String {
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

pub(crate) fn compact_title_for_pod(title: &str) -> String {
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

pub(crate) fn domain_label(url: &str) -> Option<String> {
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

pub(crate) fn title_case_words(value: &str) -> String {
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

pub(crate) fn slugify(value: &str) -> String {
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

pub(crate) fn score_pod_route(
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

pub(crate) fn candidate_placement_is_visible(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> bool {
    authorize_harness(
        store,
        ctx,
        HarnessCapability::CandidateSubmission,
        Some(pod_id),
    )
    .is_ok()
        || authorize_local_pod_curation(store, ctx, pod_id).is_ok()
}

pub(crate) fn candidate_submission_is_visible(
    store: &InMemoryStore,
    ctx: &AuthContext,
    harness: Option<&AgentHarness>,
    submission: &CandidateSubmission,
) -> bool {
    match submission.target {
        CandidateSubmissionTarget::User { user_id, .. } => {
            ctx.user_id == Some(user_id)
                && harness.is_some_and(|harness| {
                    harness.kind == AgentHarnessKind::Interactive
                        && harness.grant.pod_ids.is_none()
                        && harness
                            .grant
                            .capabilities
                            .contains(&HarnessCapability::CandidateSubmission)
                })
        }
        CandidateSubmissionTarget::PodPlacements { ref placements, .. } => {
            !placements.is_empty()
                && placements
                    .iter()
                    .all(|placement| candidate_placement_is_visible(store, ctx, placement.pod_id))
        }
        CandidateSubmissionTarget::PersonalDiscovery {
            user_id, task_id, ..
        } => {
            if authorize_personal_discovery_management(store, ctx).is_ok()
                && ctx.user_id == Some(user_id)
            {
                return true;
            }
            harness.is_some_and(|harness| {
                authorize_personal_discovery_execution(store, ctx).is_ok()
                    && (submission.submitted_by == harness.id
                        || store.discovery_tasks.get(&task_id).is_some_and(|task| {
                            matches!(
                                &task.state,
                                DiscoveryTaskState::Leased(lease)
                                    if lease.harness_id == harness.id
                                        && lease.expires_at > Utc::now()
                            )
                        }))
            })
        }
    }
}

pub(crate) fn validate_candidate_submission(
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
    let harness =
        harness_for_context(store, ctx)?.ok_or(AgentToolsError::CandidateHarnessRequired)?;
    match &request.target {
        CandidateSubmissionRequestTarget::PersonalDiscovery { .. } => {
            authorize_personal_discovery_execution(store, ctx)?;
        }
        CandidateSubmissionRequestTarget::User { .. }
        | CandidateSubmissionRequestTarget::PodPlacements { .. } => {
            authorize_harness(store, ctx, HarnessCapability::CandidateSubmission, None)?;
        }
    }
    if matches!(
        request.target,
        CandidateSubmissionRequestTarget::User { .. }
    ) && (harness.kind != AgentHarnessKind::Interactive
        || harness.grant.pod_ids.is_some()
        || ctx.user_id.is_none())
    {
        return Err(AgentToolsError::Forbidden {
            reason: "User-targeted Candidate Submission requires an unscoped interactive grant"
                .into(),
        });
    }
    let canonical_source_url = canonicalize_candidate_evidence_url(&evidence.source_url)?;
    if let Some(referrer_url) = &evidence.provenance.referrer_url {
        canonicalize_candidate_evidence_url(referrer_url)?;
    }
    resolve_media_for_store(
        store
            .submissions
            .values()
            .filter(|item| {
                item.tenant_id == ctx.tenant_id && item.canonical_url == canonical_source_url
            })
            .flat_map(|item| &item.media_references)
            .chain(
                store
                    .candidate_submissions
                    .values()
                    .filter(|submission| {
                        store
                            .candidates
                            .get(&submission.candidate_id)
                            .is_some_and(|candidate| {
                                candidate.tenant_id == ctx.tenant_id
                                    && candidate.canonical_url == canonical_source_url
                            })
                            && candidate_submission_is_visible(
                                store,
                                ctx,
                                Some(harness),
                                submission,
                            )
                            && matches!(
                                (&request.target, &submission.target),
                                (
                                    CandidateSubmissionRequestTarget::User { .. },
                                    CandidateSubmissionTarget::User { .. },
                                ) | (
                                    CandidateSubmissionRequestTarget::PodPlacements { .. },
                                    CandidateSubmissionTarget::PodPlacements { .. },
                                ) | (
                                    CandidateSubmissionRequestTarget::PersonalDiscovery { .. },
                                    CandidateSubmissionTarget::PersonalDiscovery { .. },
                                )
                            )
                    })
                    .flat_map(|submission| &submission.evidence.media_references),
            )
            .chain(&evidence.media_references),
    )?;

    let placements = request.target.placements();
    if matches!(
        request.target,
        CandidateSubmissionRequestTarget::PodPlacements { .. }
    ) && placements.is_empty()
    {
        return Err(StoreError::Validation(
            "Pod-targeted Candidate Submission requires at least one placement".into(),
        )
        .into());
    }
    let mut pod_ids = HashSet::with_capacity(placements.len());
    let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
    for placement in placements {
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

pub(crate) fn resolve_media_for_store<'a>(
    references: impl IntoIterator<Item = &'a MediaReference>,
) -> Result<Vec<MediaReference>, AgentToolsError> {
    resolve_media_evidence(references)
        .map_err(|error| StoreError::Validation(error.to_string()).into())
}

pub(crate) fn validate_candidate_task_context(
    store: &InMemoryStore,
    ctx: &AuthContext,
    harness: &AgentHarness,
    request: &CandidateSubmissionRequest,
) -> Result<(), AgentToolsError> {
    if let CandidateSubmissionRequestTarget::PersonalDiscovery { task_id, .. } = &request.target {
        let task = store
            .discovery_tasks
            .get(task_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
        if task.target.discovery_plan_id().is_none() {
            return Err(StoreError::Validation(
                "Personal Discovery result requires a Personal Discovery Task".into(),
            )
            .into());
        }
        authorize_discovery_task(store, ctx, task)?;
        if !matches!(
            &task.state,
            DiscoveryTaskState::Leased(lease)
                if lease.harness_id == harness.id && lease.expires_at > Utc::now()
        ) {
            return Err(AgentToolsError::CandidateTaskLeaseRequired);
        }
        let plan_id = task
            .target
            .discovery_plan_id()
            .expect("Personal target checked above");
        store
            .discovery_plans
            .get(&plan_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;
        return Ok(());
    }
    match request.target.task_context() {
        Some(task_context) => {
            let task = store
                .discovery_tasks
                .get(&task_context.task_id)
                .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
            let Some((pod_id, package_version)) = task.target.pod() else {
                return Err(StoreError::Validation(
                    "Pod Candidate Submission cannot use a Personal Discovery Task".into(),
                )
                .into());
            };
            authorize_harness(store, ctx, HarnessCapability::DiscoveryTasks, Some(pod_id))?;
            if package_version != task_context.package_version {
                return Err(AgentToolsError::CandidatePackageVersionMismatch);
            }
            if !request
                .target
                .placements()
                .iter()
                .any(|placement| placement.pod_id == pod_id)
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

pub(crate) fn idempotent_candidate_submission(
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

pub(crate) fn candidate_submission_matches_request(
    submission: &CandidateSubmission,
    request: &CandidateSubmissionRequest,
) -> bool {
    let target_matches = match (&submission.target, &request.target) {
        (
            CandidateSubmissionTarget::User {
                learn: stored_learn,
                interest_seed_metadata: stored_metadata,
                ..
            },
            CandidateSubmissionRequestTarget::User {
                learn: requested_learn,
                interest_seed_metadata: requested_metadata,
            },
        ) => stored_learn == requested_learn && stored_metadata == requested_metadata,
        (
            CandidateSubmissionTarget::PersonalDiscovery {
                task_id: stored_task,
                allocation_role: stored_role,
                source_facts: stored_facts,
                ..
            },
            CandidateSubmissionRequestTarget::PersonalDiscovery {
                task_id: requested_task,
                allocation_role: requested_role,
                source_facts: requested_facts,
            },
        ) => {
            stored_task == requested_task
                && stored_role == requested_role
                && stored_facts == requested_facts
        }
        (
            CandidateSubmissionTarget::PodPlacements {
                placements: stored,
                task_context: stored_task,
            },
            CandidateSubmissionRequestTarget::PodPlacements {
                placements: requested,
                task_context: requested_task,
            },
        ) => stored == requested && stored_task == requested_task,
        _ => false,
    };
    target_matches && submission.evidence == request.evidence
}

pub(crate) fn stable_candidate_uuid(namespace: &str, parts: &[&str]) -> Uuid {
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

pub(crate) fn authorize_harness_for_new_pod(
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

pub(crate) fn authorize_harness_pod_scope(
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

pub(crate) fn authorize_harness_submission_scope(
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

pub(crate) fn curation_actor(ctx: &AuthContext) -> CurationActor {
    ctx.harness_id
        .map(CurationActor::Harness)
        .or_else(|| ctx.user_id.map(CurationActor::User))
        .unwrap_or(CurationActor::NodeAgent)
}

pub(crate) fn authorize_local_pod_curation(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, HarnessCapability::PodCuration, Some(pod_id))?;
    let pod = store
        .pods
        .get(&pod_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
    store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
    let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
    if pod
        .origin_node_id
        .is_some_and(|origin_node_id| origin_node_id != local_node_id)
    {
        return Err(AgentToolsError::Forbidden {
            reason: format!("remote Pod {pod_id} cannot receive local curation"),
        });
    }
    Ok(())
}

pub(crate) fn candidate_submissions_for(
    store: &InMemoryStore,
    candidate_id: CandidateId,
) -> Vec<CandidateSubmission> {
    let mut submissions = store
        .candidate_submissions
        .values()
        .filter(|submission| submission.candidate_id == candidate_id)
        .cloned()
        .collect::<Vec<_>>();
    submissions.sort_by_key(|submission| (submission.created_at, submission.id));
    submissions
}

pub(crate) struct MergedCandidateProposal {
    pub(crate) pod_id: PodId,
    pub(crate) reason: CurationRationale,
    pub(crate) confidence: CandidateConfidence,
    pub(crate) source_submission_ids: Vec<CandidateSubmissionId>,
}

pub(crate) fn merged_candidate_proposals(
    submissions: &[CandidateSubmission],
) -> Result<Vec<MergedCandidateProposal>, AgentToolsError> {
    let mut proposals: BTreeMap<PodId, MergedCandidateProposal> = BTreeMap::new();
    for submission in submissions {
        for placement in submission.target.placements() {
            let rationale = CurationRationale::new(placement.reason.clone())?;
            let entry =
                proposals
                    .entry(placement.pod_id)
                    .or_insert_with(|| MergedCandidateProposal {
                        pod_id: placement.pod_id,
                        reason: rationale.clone(),
                        confidence: placement.confidence,
                        source_submission_ids: Vec::new(),
                    });
            if placement.confidence.value() > entry.confidence.value() {
                entry.reason = rationale;
                entry.confidence = placement.confidence;
            }
            entry.source_submission_ids.push(submission.id);
        }
    }
    Ok(proposals.into_values().collect())
}

pub(crate) fn trusted_placement_confidence(
    store: &InMemoryStore,
    submissions: &[CandidateSubmission],
    pod_id: PodId,
) -> Option<CandidateConfidence> {
    submissions
        .iter()
        .filter(|submission| {
            submission
                .target
                .task_context()
                .and_then(|context| store.discovery_tasks.get(&context.task_id))
                .is_some_and(|task| {
                    task.target
                        .pod()
                        .is_some_and(|(task_pod_id, _)| task_pod_id == pod_id)
                })
        })
        .flat_map(|submission| submission.target.placements())
        .filter(|placement| placement.pod_id == pod_id)
        .map(|placement| placement.confidence)
        .max_by(|left, right| left.value().total_cmp(&right.value()))
}

pub(crate) fn ensure_content_item(
    store: &mut InMemoryStore,
    candidate: &Candidate,
    submissions: &[CandidateSubmission],
    authorized_submission_ids: &[CandidateSubmissionId],
    now: chrono::DateTime<Utc>,
) -> Result<ContentItem, AgentToolsError> {
    if let Some(existing) = store
        .submissions
        .values()
        .find(|item| {
            item.tenant_id == candidate.tenant_id && item.canonical_url == candidate.canonical_url
        })
        .cloned()
    {
        return Ok(ContentItem::from(&existing));
    }
    let authorized_submissions = submissions
        .iter()
        .filter(|submission| authorized_submission_ids.contains(&submission.id))
        .collect::<Vec<_>>();
    let evidence = authorized_submissions.first().copied().ok_or_else(|| {
        StoreError::Validation(
            "Candidate placement has no explicitly authorized submission evidence".into(),
        )
    })?;
    let reference = CandidateReference::from_submissions(authorized_submissions.iter().copied())
        .expect("authorized Candidate placement has submission evidence");
    let domain = Url::parse(&candidate.canonical_url)
        .map_err(|error| AgentToolsError::BadUrl(error.to_string()))?
        .domain()
        .unwrap_or("unknown")
        .to_string();
    let submitted_by = store
        .agent_harnesses
        .get(&evidence.submitted_by)
        .map(|harness| harness.user_id);
    let media_references = resolve_media_for_store(
        submissions
            .iter()
            .filter(|submission| authorized_submission_ids.contains(&submission.id))
            .flat_map(|submission| &submission.evidence.media_references),
    )?;
    let item = Submission {
        id: stable_candidate_uuid("content-item", &[&candidate.id.to_string()]),
        tenant_id: candidate.tenant_id,
        url: reference.source_url,
        canonical_url: candidate.canonical_url.clone(),
        title: reference
            .source_metadata
            .title
            .clone()
            .unwrap_or_else(|| candidate.canonical_url.clone()),
        source_metadata: reference.source_metadata,
        description: reference.permitted_excerpt,
        domain,
        submitted_by,
        discovered_by_crawler: false,
        submitter_note: None,
        summary: reference.summary,
        provenance: authorized_submissions
            .into_iter()
            .map(|submission| submission.evidence.provenance.clone())
            .collect(),
        media_references,
        tags: reference.tags,
        embedding: None,
        created_at: now,
        origin_event_id: None,
    };
    store.submissions.insert(item.id, item.clone());
    Ok(ContentItem::from(&item))
}

pub(crate) fn merge_source_metadata(
    retained: &mut CandidateSourceMetadata,
    additional: &CandidateSourceMetadata,
) {
    if retained.title.is_none() {
        retained.title.clone_from(&additional.title);
    }
    if retained.author.is_none() {
        retained.author.clone_from(&additional.author);
    }
    if retained.published_at.is_none() {
        retained.published_at = additional.published_at;
    }
}
