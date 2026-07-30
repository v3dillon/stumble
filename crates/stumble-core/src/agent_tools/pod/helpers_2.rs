use super::super::prelude::*;
use super::super::*;

pub(crate) fn enrich_accepted_content_item(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
) -> Result<(), AgentToolsError> {
    let Some(item_id) = store
        .submissions
        .values()
        .find(|item| {
            item.tenant_id == candidate.tenant_id && item.canonical_url == candidate.canonical_url
        })
        .map(|item| item.id)
    else {
        return Ok(());
    };
    let existing = store
        .submissions
        .get(&item_id)
        .ok_or_else(|| StoreError::NotFound("Content Item".into()))?
        .clone();
    let content_item_id = ContentItemId::from(item_id);
    let accepted_pod_ids = store
        .accepted_placement_projections
        .values()
        .filter(|placement| placement.content_item_id == content_item_id)
        .map(|placement| placement.pod_id)
        .collect::<HashSet<_>>();
    let accepted_submissions = store
        .candidate_submissions
        .values()
        .filter(|submission| {
            submission.candidate_id == candidate.id
                && submission
                    .target
                    .placements()
                    .iter()
                    .any(|placement| accepted_pod_ids.contains(&placement.pod_id))
        })
        .collect::<Vec<_>>();
    let reference = CandidateReference::from_submissions(accepted_submissions.iter().copied());
    let resolved = resolve_media_for_store(
        existing.media_references.iter().chain(
            accepted_submissions
                .iter()
                .flat_map(|submission| &submission.evidence.media_references),
        ),
    )?;
    let item = store
        .submissions
        .get_mut(&item_id)
        .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
    if let Some(reference) = reference {
        merge_source_metadata(&mut item.source_metadata, &reference.source_metadata);
        if let Some(title) = &item.source_metadata.title {
            item.title.clone_from(title);
        }
        if item.description.is_none() {
            item.description = reference.permitted_excerpt;
        }
        if item.summary.is_none() {
            item.summary = reference.summary;
        }
        extend_unique(&mut item.tags, reference.tags);
        extend_unique(
            &mut item.provenance,
            accepted_submissions
                .iter()
                .map(|submission| submission.evidence.provenance.clone()),
        );
    }
    item.media_references = resolved;
    if *item == existing {
        return Ok(());
    }

    let node = store.node_for_tenant(ctx.tenant_id)?;
    let mut pods = store
        .accepted_placement_projections
        .values()
        .filter(|placement| {
            placement.content_item_id == content_item_id && placement.origin_node_id == node.id
        })
        .filter_map(|placement| store.pods.get(&placement.pod_id).cloned())
        .collect::<Vec<_>>();
    pods.sort_by(|left, right| left.slug.cmp(&right.slug).then(left.id.cmp(&right.id)));
    let content_item = store
        .submissions
        .get(&item_id)
        .expect("accepted Content Item remains present")
        .clone();
    let now = Utc::now();
    for pod in pods {
        let payload = ContentItemMetadataUpdatedPayload {
            metadata_update: ContentItemMetadataUpdate {
                content_item_id,
                source_metadata: content_item.source_metadata.clone(),
                permitted_excerpt: content_item.description.clone(),
                summary: content_item.summary.clone(),
                tags: content_item.tags.clone(),
                provenance: content_item.provenance.clone(),
                media_references: content_item.media_references.clone(),
            },
        };
        let event = sign_public_event(
            &node,
            FederatedPodEventType::ContentItemMetadataUpdated.as_wire(),
            &pod.slug,
            serde_json::to_value(payload).map_err(|error| {
                StoreError::Validation(format!("metadata update cannot be signed: {error}"))
            })?,
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(store, pod.id, now)?;
    }
    Ok(())
}

pub(crate) fn accept_placement(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    placement: &PodPlacement,
) -> Result<(), AgentToolsError> {
    let content_item_id = placement.content_item_id.ok_or_else(|| {
        StoreError::Validation("Accepted Placement requires a Content Item".into())
    })?;
    if !store.submission_pods.iter().any(|existing| {
        existing.pod_id == placement.pod_id && existing.submission_id == Uuid::from(content_item_id)
    }) {
        store.submission_pods.push(SubmissionPod {
            submission_id: content_item_id.into(),
            pod_id: placement.pod_id,
            created_at: placement.updated_at,
        });
    }
    let pod = store
        .pods
        .get(&placement.pod_id)
        .cloned()
        .ok_or_else(|| StoreError::NotFound(format!("Pod {}", placement.pod_id)))?;
    let item = store
        .submissions
        .get(&Uuid::from(content_item_id))
        .cloned()
        .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
    let node = store.node_for_tenant(ctx.tenant_id)?;
    let projection = AcceptedPlacementProjection {
        content_item_id,
        pod_id: placement.pod_id,
        reason: placement.reason.clone(),
        curation_path: placement.curation_path,
        origin_node_id: node.id,
        accepted_at: placement.updated_at,
    };
    store
        .accepted_placement_projections
        .insert((content_item_id, placement.pod_id), projection.clone());
    let event = sign_public_event(
        &node,
        "content_item_placed",
        &pod.slug,
        json!({
            "content_item": ContentItem::from(&item),
            "accepted_placement": projection,
        }),
        store.latest_event_hash(&pod.slug),
    )?;
    store.event_log.push(event);
    refresh_public_pod_announcement_if_needed(store, placement.pod_id, placement.updated_at)?;
    Ok(())
}

pub(crate) fn accept_candidate_placement(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
    placement: &PodPlacement,
) -> Result<(), AgentToolsError> {
    accept_placement(store, ctx, placement)?;
    if let Some(candidate) = store.candidates.get_mut(&candidate.id) {
        candidate.review_state = CandidateReviewState::Accepted;
    }
    Ok(())
}

pub(crate) fn candidate_curation_result(
    store: &InMemoryStore,
    candidate_id: CandidateId,
) -> Result<CandidateCurationResult, AgentToolsError> {
    let candidate = store
        .candidates
        .get(&candidate_id)
        .cloned()
        .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
    let mut placements = store
        .pod_placements
        .values()
        .filter(|placement| placement.candidate_id == candidate_id)
        .cloned()
        .collect::<Vec<_>>();
    placements.sort_by_key(|placement| placement.pod_id);
    let content_item = placements
        .iter()
        .find_map(|placement| placement.content_item_id)
        .and_then(|content_item_id| {
            store
                .submissions
                .get(&Uuid::from(content_item_id))
                .map(ContentItem::from)
        });
    Ok(CandidateCurationResult {
        candidate,
        content_item,
        placements,
    })
}

pub(crate) fn verify_portable_package_history(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
) -> Result<(), AgentToolsError> {
    let events = verified_portable_package_events(store, files)?;
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

pub(crate) fn verify_portable_package_history_for_base(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
    base: &PodPackage,
) -> Result<(), AgentToolsError> {
    let events = verified_portable_package_events(store, files)?;
    let has_signed_base = events.iter().any(|event| {
        event
            .payload_json
            .get("package")
            .and_then(|value| serde_json::from_value::<PodPackage>(value.clone()).ok())
            .is_some_and(|package| package == *base)
    });
    if !has_signed_base {
        return Err(StoreError::Validation(
            "events.jsonl does not contain the signed base Package version".to_string(),
        )
        .into());
    }
    Ok(())
}

pub(crate) fn verified_portable_package_events(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
) -> Result<Vec<EventLog>, AgentToolsError> {
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
    for event in &events {
        let public_key = store
            .node_identities
            .get(&event.author_node_id)
            .filter(|node| node.tenant_id == event.tenant_id)
            .map(|node| node.public_key.as_str())
            .or_else(|| {
                store
                    .trusted_peers
                    .get(&event.author_node_id)
                    .filter(|peer| peer.enabled)
                    .map(|peer| peer.public_key.as_str())
            })
            .or_else(|| {
                store.known_pod_announcements.values().find_map(|known| {
                    (known.announcement.origin_node_id == event.author_node_id)
                        .then_some(known.announcement.signer.public_key.as_str())
                })
            })
            .ok_or(StoreError::UntrustedPeer)?;
        if !verify_event(event, public_key).map_err(|_| StoreError::InvalidSignature)? {
            return Err(StoreError::InvalidSignature.into());
        }
    }
    Ok(events)
}

pub(crate) fn ensure_package_base_version(
    existing: &PodPackage,
    base_version: PackageVersion,
) -> Result<(), AgentToolsError> {
    if PackageVersion::new(existing.version)
        .map_err(|error| StoreError::Validation(error.to_string()))?
        != base_version
    {
        return Err(StoreError::Validation("Package Revision base version is stale".into()).into());
    }
    Ok(())
}

pub(crate) fn complete_package_patch(contents: &PodPackageContents) -> SkillPackPatch {
    SkillPackPatch {
        context_md: Some(contents.context_md.clone()),
        pod_yaml: None,
        skill_md: Some(contents.skill_md.clone()),
        sources_yaml: Some(contents.sources_yaml.clone()),
        filters_yaml: Some(contents.filters_yaml.clone()),
        examples_good_md: Some(contents.examples_good_md.clone()),
        examples_bad_md: Some(contents.examples_bad_md.clone()),
    }
}

pub(crate) fn ensure_direct_package_revision_allowed(
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

pub(crate) fn ensure_direct_package_revision_allowed_for_origin(
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

pub(crate) fn package_contents_match(
    package: &PodSkillPack,
    contents: &PodPackageContents,
) -> bool {
    package.context_md == contents.context_md
        && package.skill_md == contents.skill_md
        && package.sources_yaml == contents.sources_yaml
        && package.filters_yaml == contents.filters_yaml
        && package.examples_good_md == contents.examples_good_md
        && package.examples_bad_md == contents.examples_bad_md
}

pub(crate) fn origin_placement_identity(
    placement: &AcceptedPlacementProjection,
) -> (ContentItemId, PodId, NodeIdentityId, chrono::DateTime<Utc>) {
    (
        placement.content_item_id,
        placement.pod_id,
        placement.origin_node_id,
        placement.accepted_at,
    )
}

pub(crate) fn visibility_exposure(visibility: &Visibility) -> u8 {
    match visibility {
        Visibility::Private => 0,
        Visibility::InviteOnly => 1,
        Visibility::Public => 2,
    }
}

pub(crate) fn validate_creation_package_locked(
    store: &InMemoryStore,
    ctx: &AuthContext,
    package: &PodCreationPackage,
) -> Result<(), AgentToolsError> {
    match package {
        PodCreationPackage::Default => Ok(()),
        PodCreationPackage::Initial { package } => {
            let report = validate_pod_package_contents(package);
            if report.valid {
                Ok(())
            } else {
                Err(StoreError::Validation(report.errors.join(", ")).into())
            }
        }
        PodCreationPackage::Derived { source_package } => {
            let source = store
                .pod_package_versions
                .values()
                .find(|candidate| candidate.id == source_package.id)
                .ok_or_else(|| StoreError::NotFound("source Pod Package".into()))?;
            let source_pod = store
                .pods
                .get(&source.pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {}", source.pod_id)))?;
            store.assert_tenant(source_pod.tenant_id, ctx.tenant_id)?;
            authorize_harness(
                store,
                ctx,
                HarnessCapability::PackageManagement,
                Some(source_pod.id),
            )?;
            if source != source_package {
                return Err(StoreError::Validation(
                    "derived source Pod Package does not match stored provenance".into(),
                )
                .into());
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PodCreationMode {
    Canonical,
    SimpleCreate,
    PrivatePackage,
    LegacyPublic,
}

impl PodCreationMode {
    const fn event_type(self) -> &'static str {
        match self {
            Self::PrivatePackage => "private_pod_package_created",
            Self::Canonical | Self::SimpleCreate | Self::LegacyPublic => "pod_created",
        }
    }

    const fn records_audit(self) -> bool {
        !matches!(self, Self::LegacyPublic)
    }
}

pub(crate) fn create_pod_lifecycle_locked(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    request: CreatePodLifecycleRequest,
    proposer: Option<AgentHarnessId>,
    mode: PodCreationMode,
) -> Result<CreatedPodPackage, AgentToolsError> {
    let mut staged = store.clone();
    let created = stage_pod_lifecycle(&mut staged, ctx, request, proposer, mode)?;
    *store = staged;
    Ok(created)
}

pub(crate) fn stage_pod_lifecycle(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    request: CreatePodLifecycleRequest,
    proposer: Option<AgentHarnessId>,
    mode: PodCreationMode,
) -> Result<CreatedPodPackage, AgentToolsError> {
    validate_creation_package_locked(store, ctx, &request.package)?;
    if store
        .pods
        .values()
        .any(|pod| pod.slug == request.pod.slug && pod.tenant_id == ctx.tenant_id)
    {
        return Err(StoreError::Duplicate(format!("pod {}", request.pod.slug)).into());
    }
    let node = store.node_for_tenant(ctx.tenant_id)?;
    let proposer_user_id = || {
        proposer.and_then(|id| {
            store
                .agent_harnesses
                .get(&id)
                .map(|harness| harness.user_id)
        })
    };
    let owner_id = match mode {
        PodCreationMode::PrivatePackage => Some(ctx.user_id.ok_or_else(|| {
            StoreError::Validation("private Pod Package requires an owner".to_string())
        })?),
        PodCreationMode::Canonical => Some(
            proposer_user_id()
                .or(ctx.user_id)
                .ok_or_else(|| StoreError::Validation("Pod creation requires an owner".into()))?,
        ),
        PodCreationMode::SimpleCreate => Some(
            proposer_user_id()
                .or(ctx.user_id)
                .or_else(|| local_owner_user_id(store))
                .ok_or_else(|| StoreError::Validation("Pod creation requires an owner".into()))?,
        ),
        PodCreationMode::LegacyPublic => proposer_user_id(),
    };
    let now = Utc::now();
    let pod = Pod {
        id: Uuid::now_v7(),
        tenant_id: ctx.tenant_id,
        name: request.pod.name,
        slug: request.pod.slug,
        description: request.pod.description,
        visibility: request.pod.visibility,
        created_by: owner_id,
        created_at: now,
        origin_node_id: Some(node.id),
    };
    let mut package = match request.package {
        PodCreationPackage::Default => default_skill_pack(&pod),
        PodCreationPackage::Initial { package } => PodSkillPack {
            id: Uuid::now_v7(),
            pod_id: pod.id,
            version: 1,
            context_md: package.context_md,
            pod_yaml: format!(
                "name: {}\nslug: {}\ndescription: {}\nvisibility: {}\n",
                pod.name,
                pod.slug,
                pod.description,
                match pod.visibility {
                    Visibility::Public => "public",
                    Visibility::InviteOnly => "invite_only",
                    Visibility::Private => "private",
                }
            ),
            skill_md: package.skill_md,
            sources_yaml: package.sources_yaml,
            filters_yaml: package.filters_yaml,
            examples_good_md: package.examples_good_md,
            examples_bad_md: package.examples_bad_md,
            owner_id,
            proposer_harness_id: proposer,
            created_at: now,
            updated_at: now,
        },
        PodCreationPackage::Derived { source_package } => fork_skill_pack(&source_package, &pod),
    };
    package.version = 1;
    package.proposer_harness_id = proposer;
    store.pods.insert(pod.id, pod.clone());
    store.pod_rules.insert(
        pod.id,
        PodRules {
            pod_id: pod.id,
            blocked_topics: Vec::new(),
            blocked_domains: Vec::new(),
            auto_promote_crawler_candidates: false,
            federate_sources: pod.visibility == Visibility::Public,
        },
    );
    if let Some(owner_id) = owner_id {
        store.pod_roles.push(PodRoleAssignment {
            user_id: owner_id,
            pod_id: pod.id,
            role: PodRole::Owner,
            created_at: now,
        });
    }
    store.insert_pod_package_version(package.clone())?;
    store.pod_skill_packs.insert(pod.id, package.clone());
    let event = sign_public_event(
        &node,
        mode.event_type(),
        &pod.slug,
        json!({"pod": pod, "package": package}),
        store.latest_event_hash(&pod.slug),
    )?;
    store.event_log.push(event);
    if mode.records_audit() {
        record_harness_write(store, ctx, HarnessWriteOperation::CreatePod, Some(pod.id));
    }
    Ok(CreatedPodPackage { pod, package })
}

pub(crate) fn pod_roles_value(store: &InMemoryStore, pod_id: PodId) -> serde_json::Value {
    let mut roles = store
        .pod_roles
        .iter()
        .filter(|assignment| assignment.pod_id == pod_id)
        .cloned()
        .collect::<Vec<_>>();
    roles.sort_by_key(|assignment| {
        (
            assignment.user_id,
            match assignment.role {
                PodRole::Owner => 0,
                PodRole::Curator => 1,
            },
        )
    });
    json!(roles)
}

pub(crate) fn authorize_pod_role_owner(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, HarnessCapability::PodCuration, Some(pod_id))?;
    if ctx.user_id.is_some_and(|user_id| {
        store.pod_roles.iter().any(|assignment| {
            assignment.user_id == user_id
                && assignment.pod_id == pod_id
                && assignment.role == PodRole::Owner
        })
    }) {
        Ok(())
    } else {
        Err(AgentToolsError::Forbidden {
            reason: format!("User is not an Owner of Pod {pod_id}"),
        })
    }
}

pub(crate) fn ensure_child_pod_scope(
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

pub(crate) fn normalize_pod_ids(mut pod_ids: Vec<PodId>) -> Vec<PodId> {
    pod_ids.sort();
    pod_ids.dedup();
    pod_ids
}

pub(crate) fn route_tokens(text: &str) -> Vec<String> {
    discovery_tokens(text)
}
