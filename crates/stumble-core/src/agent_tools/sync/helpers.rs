use super::super::prelude::*;
use super::super::*;

pub(crate) fn discard_replayed_events(
    store: &InMemoryStore,
    cursor: Option<&str>,
    snapshot: &mut FederationPodSnapshot,
) -> Result<(), AgentToolsError> {
    let mut previous_hash = snapshot
        .events
        .first()
        .and_then(|event| event.previous_event_hash.clone());
    for event in &snapshot.events {
        if event.author_node_id != snapshot.node.node_id
            || event.pod_slug != snapshot.manifest.pod.slug
            || event.previous_event_hash != previous_hash
            || !is_subscription_projection_event(&event.event_type)
            || !verify_event(event, &snapshot.node.public_key)?
        {
            return Err(StoreError::InvalidSignature.into());
        }
        previous_hash = Some(event.content_hash.clone());
    }
    let Some(cursor) = cursor else {
        return Ok(());
    };
    if snapshot
        .events
        .first()
        .is_none_or(|event| event.previous_event_hash.as_deref() == Some(cursor))
    {
        return Ok(());
    }
    if let Some(cursor_index) = snapshot
        .events
        .iter()
        .position(|event| event.content_hash == cursor)
    {
        snapshot.events.drain(..=cursor_index);
        return Ok(());
    }
    let is_complete_retry = snapshot
        .events
        .last()
        .is_some_and(|event| event.content_hash == cursor)
        && snapshot.events.iter().all(|event| {
            store.event_log.iter().any(|existing| {
                existing.event_id == event.event_id && existing.content_hash == event.content_hash
            })
        });
    if is_complete_retry {
        snapshot.events.clear();
        return Ok(());
    }
    Err(StoreError::Validation("signed Pod Event chain is discontinuous".to_string()).into())
}

/// Validates and canonicalizes a direct public Pod address before outbound I/O.
///
/// # Errors
///
/// Returns an error unless the address uses HTTPS (or loopback HTTP) and has
/// the canonical `/federation/pods/<slug>` shape.
pub fn canonical_public_pod_url(value: &str) -> Result<String, AgentToolsError> {
    crate::pod_announcement::canonical_public_pod_url(value).map_err(Into::into)
}

pub(crate) fn validate_federation_snapshot(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    expected_previous_hash: Option<&str>,
    snapshot: &FederationPodSnapshot,
) -> Result<(), AgentToolsError> {
    let pod = &snapshot.manifest.pod;
    validate_protocol_version(&snapshot.node.supported_protocol_version)?;
    if pod.visibility != Visibility::Public || pod.origin_node_id != Some(snapshot.node.node_id) {
        return Err(StoreError::Validation(
            "federation snapshot does not describe an authoritative public Pod".to_string(),
        )
        .into());
    }
    validate_remote_pod_identity(store, tenant_id, snapshot)?;
    let mut previous_hash = expected_previous_hash.map(str::to_string).or_else(|| {
        snapshot
            .events
            .first()
            .filter(|event| event.event_type == "pod_published")
            .and_then(|event| event.previous_event_hash.clone())
    });
    for event in &snapshot.events {
        if event.pod_slug != pod.slug
            || event.author_node_id != snapshot.node.node_id
            || !is_subscription_projection_event(&event.event_type)
        {
            return Err(StoreError::Validation(
                "event is outside the subscribed public Pod stream".to_string(),
            )
            .into());
        }
        if event.previous_event_hash != previous_hash {
            return Err(StoreError::Validation(
                "signed Pod Event chain is discontinuous".to_string(),
            )
            .into());
        }
        if !verify_event(event, &snapshot.node.public_key)? {
            return Err(StoreError::InvalidSignature.into());
        }
        validate_imported_event_payload(event)?;
        previous_hash = Some(event.content_hash.clone());
    }
    if previous_hash != snapshot.manifest.latest_known_event_hash {
        return Err(StoreError::Validation(
            "federation snapshot does not reach the manifest event pointer".to_string(),
        )
        .into());
    }

    let signed_packages = snapshot
        .events
        .iter()
        .filter_map(|event| event.payload_json.get("package"))
        .map(|value| serde_json::from_value::<PodPackage>(value.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StoreError::Validation("signed Pod Package is malformed".to_string()))?;
    validate_signed_package_versions(store, tenant_id, snapshot, &signed_packages)?;
    Ok(())
}

pub(crate) fn validate_remote_pod_identity(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    snapshot: &FederationPodSnapshot,
) -> Result<(), AgentToolsError> {
    let remote = &snapshot.manifest.pod;
    let origin_node_id = snapshot.node.node_id;
    if store.pods.values().any(|local| {
        local.tenant_id == tenant_id
            && local.slug == remote.slug
            && local.origin_node_id != Some(origin_node_id)
    }) {
        return Err(StoreError::Duplicate(format!(
            "local Pod slug {} conflicts with the subscribed Origin",
            remote.slug
        ))
        .into());
    }
    if store.pods.get(&remote.id).is_some_and(|local| {
        local.tenant_id != tenant_id
            || local.slug != remote.slug
            || local.origin_node_id != Some(origin_node_id)
    }) {
        return Err(StoreError::Duplicate(format!(
            "Origin Pod identity {} conflicts with local state",
            remote.id
        ))
        .into());
    }
    Ok(())
}

pub(crate) fn validate_signed_package_versions(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    snapshot: &FederationPodSnapshot,
    signed_packages: &[PodPackage],
) -> Result<(), AgentToolsError> {
    let remote_pod = &snapshot.manifest.pod;
    let local_pod = store.pods.values().find(|local| {
        local.tenant_id == tenant_id
            && local.slug == remote_pod.slug
            && local.origin_node_id == Some(snapshot.node.node_id)
    });
    let local_package = local_pod.and_then(|pod| store.pod_skill_packs.get(&pod.id));
    let mut verified_version = local_package.map(|package| package.version);
    let mut immutable_versions = BTreeMap::new();
    if let Some(package) = local_package {
        immutable_versions.insert(
            package.version,
            normalized_package_value(package, package.pod_id)?,
        );
    }
    let projected_pod_id = local_pod.map_or(remote_pod.id, |pod| pod.id);
    for package in signed_packages {
        PackageVersion::new(package.version)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        if package.pod_id != remote_pod.id || !validate_skill_pack(package).valid {
            return Err(StoreError::Validation(
                "signed Pod Package is invalid or belongs to another Pod".to_string(),
            )
            .into());
        }
        if verified_version.is_some_and(|version| package.version < version) {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot move backwards".to_string(),
            )
            .into());
        }
        let value = normalized_package_value(package, projected_pod_id)?;
        if immutable_versions
            .get(&package.version)
            .is_some_and(|existing| existing != &value)
        {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot be reused with different contents".to_string(),
            )
            .into());
        }
        immutable_versions.insert(package.version, value);
        verified_version = Some(package.version);
    }
    PackageVersion::new(snapshot.manifest.skill_pack_version)
        .map_err(|error| StoreError::Validation(error.to_string()))?;
    if verified_version != Some(snapshot.manifest.skill_pack_version) {
        return Err(StoreError::Validation(
            "manifest Pod Package version lacks a matching signed event".to_string(),
        )
        .into());
    }
    Ok(())
}

pub(crate) fn normalized_package_value(
    package: &PodPackage,
    projected_pod_id: PodId,
) -> Result<serde_json::Value, AgentToolsError> {
    let mut package = package.clone();
    package.pod_id = projected_pod_id;
    package.owner_id = None;
    package.proposer_harness_id = None;
    serde_json::to_value(package).map_err(|error| {
        StoreError::Validation(format!("signed Pod Package cannot be compared: {error}")).into()
    })
}

pub(crate) fn validate_imported_event_payload(event: &EventLog) -> Result<(), AgentToolsError> {
    let event_type = FederatedPodEventType::from_wire(&event.event_type)
        .ok_or_else(|| StoreError::Validation("event is not synchronization-safe".to_string()))?;
    match event_type {
        FederatedPodEventType::PodCreated => {
            imported_event_payload::<Pod>(event, "pod")?;
            imported_event_payload::<PodPackage>(event, "package")?;
        }
        FederatedPodEventType::PodPublished => {
            imported_event_payload::<Pod>(event, "pod")?;
            imported_event_payload::<PodPackage>(event, "package")?;
        }
        FederatedPodEventType::PodSkillPackUpdated
        | FederatedPodEventType::PodPackageImported
        | FederatedPodEventType::PodPackageForked => {
            imported_event_payload::<PodPackage>(event, "package")?;
        }
        FederatedPodEventType::ContentItemPlaced => {
            imported_event_payload::<ContentItem>(event, "content_item")?;
            imported_event_payload::<AcceptedPlacementProjection>(event, "accepted_placement")?;
        }
        FederatedPodEventType::ContentItemMetadataUpdated => {
            let payload = imported_event_body::<ContentItemMetadataUpdatedPayload>(event)?;
            resolve_media_for_store(&payload.metadata_update.media_references)?;
        }
        FederatedPodEventType::PlacementTombstoned => {
            imported_event_payload::<PlacementTombstone>(event, "placement_tombstone")?;
        }
        FederatedPodEventType::LegacyLinkRemoved => {
            imported_event_payload::<SubmissionId>(event, "submission_id")?;
        }
        FederatedPodEventType::LegacyLinkSubmitted => {
            return Err(
                StoreError::Validation("event is not synchronization-safe".to_string()).into(),
            )
        }
    }
    Ok(())
}

pub(crate) fn project_snapshot_events(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    events: &[EventLog],
) -> Result<usize, AgentToolsError> {
    let mut imported = 0;
    for event in events {
        if store.event_log.iter().any(|existing| {
            existing.event_id == event.event_id || existing.content_hash == event.content_hash
        }) {
            continue;
        }
        let mut imported_event = event.clone();
        imported_event.tenant_id = ctx.tenant_id;
        imported_event.imported_from_peer_id = None;
        imported_event.verified = true;
        if is_subscription_projection_event(&imported_event.event_type) {
            project_imported_public_event(store, ctx, &imported_event)?;
        }
        store.event_log.push(imported_event);
        imported += 1;
    }
    Ok(imported)
}

pub(crate) fn is_subscription_projection_event(event_type: &str) -> bool {
    matches!(
        FederatedPodEventType::from_wire(event_type),
        Some(
            FederatedPodEventType::PodCreated
                | FederatedPodEventType::PodPublished
                | FederatedPodEventType::PodSkillPackUpdated
                | FederatedPodEventType::PodPackageImported
                | FederatedPodEventType::PodPackageForked
                | FederatedPodEventType::ContentItemPlaced
                | FederatedPodEventType::ContentItemMetadataUpdated
                | FederatedPodEventType::PlacementTombstoned
        )
    )
}

pub(crate) fn project_imported_public_event(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
) -> Result<(), AgentToolsError> {
    let Some(event_type) = FederatedPodEventType::from_wire(&event.event_type) else {
        return Ok(());
    };
    match event_type {
        FederatedPodEventType::PodCreated => {
            let pod = imported_event_payload::<Pod>(event, "pod")?;
            let local_pod_id = project_imported_pod(store, ctx, event.author_node_id, pod)?;
            let mut package = imported_event_payload::<PodPackage>(event, "package")?;
            project_imported_package(store, local_pod_id, &mut package)?;
        }
        FederatedPodEventType::PodPublished => {
            let pod = imported_event_payload::<Pod>(event, "pod")?;
            let local_pod_id = project_imported_pod(store, ctx, event.author_node_id, pod)?;
            let mut package = imported_event_payload::<PodPackage>(event, "package")?;
            project_imported_package(store, local_pod_id, &mut package)?;
        }
        FederatedPodEventType::PodSkillPackUpdated
        | FederatedPodEventType::PodPackageImported
        | FederatedPodEventType::PodPackageForked => {
            let mut package = imported_event_payload::<PodPackage>(event, "package")?;
            let local_pod_id = synchronized_origin_pod_id(store, ctx, event)?;
            project_imported_package(store, local_pod_id, &mut package)?;
        }
        FederatedPodEventType::LegacyLinkSubmitted => {
            let submission = imported_event_payload::<Submission>(event, "submission")?;
            project_imported_submission(store, ctx, event, submission)?;
        }
        FederatedPodEventType::ContentItemPlaced => {
            let content_item = imported_event_payload::<ContentItem>(event, "content_item")?;
            let content_item_id =
                project_imported_submission(store, ctx, event, content_item.into_legacy_record())?;
            let mut projection =
                imported_event_payload::<AcceptedPlacementProjection>(event, "accepted_placement")?;
            let local_pod_id = synchronized_origin_pod_id(store, ctx, event)?;
            projection.content_item_id = content_item_id;
            projection.pod_id = local_pod_id;
            projection.origin_node_id = event.author_node_id;
            store
                .accepted_placement_projections
                .insert((content_item_id, local_pod_id), projection);
        }
        FederatedPodEventType::ContentItemMetadataUpdated => {
            let payload = imported_event_body::<ContentItemMetadataUpdatedPayload>(event)?;
            let update = payload.metadata_update;
            let media_references = resolve_media_for_store(&update.media_references)?;
            let key = FederatedContentItemKey::new(
                ctx.tenant_id,
                event.author_node_id,
                update.content_item_id,
            );
            let local_content_item_id = store
                .federated_content_item_ids
                .get(&key)
                .copied()
                .ok_or_else(|| StoreError::NotFound("synchronized Content Item".into()))?;
            let local_pod_id = synchronized_origin_pod_id(store, ctx, event)?;
            if !store
                .accepted_placement_projections
                .contains_key(&(local_content_item_id, local_pod_id))
            {
                return Err(StoreError::Validation(
                    "metadata update requires a synchronized Accepted Placement".into(),
                )
                .into());
            }
            let item = store
                .submissions
                .get_mut(&Uuid::from(local_content_item_id))
                .ok_or_else(|| StoreError::NotFound("synchronized Content Item".into()))?;
            merge_source_metadata(&mut item.source_metadata, &update.source_metadata);
            if item.source_metadata.title.is_some() {
                item.title = item
                    .source_metadata
                    .title
                    .clone()
                    .expect("checked source title");
            }
            if item.description.is_none() {
                item.description = update.permitted_excerpt;
            }
            if item.summary.is_none() {
                item.summary = update.summary;
            }
            extend_unique(&mut item.tags, update.tags);
            extend_unique(&mut item.provenance, update.provenance);
            item.media_references =
                resolve_media_for_store(item.media_references.iter().chain(&media_references))?;
        }
        FederatedPodEventType::PlacementTombstoned => {
            let mut tombstone =
                imported_event_payload::<PlacementTombstone>(event, "placement_tombstone")?;
            if tombstone.origin_placement.origin_node_id != event.author_node_id
                || tombstone.content_reference.content_item_id
                    != tombstone.origin_placement.content_item_id
            {
                return Err(StoreError::Validation(
                    "signed Placement Tombstone does not match its Origin Placement".into(),
                )
                .into());
            }
            let origin_content_item_id = tombstone.origin_placement.content_item_id;
            let key = FederatedContentItemKey::new(
                ctx.tenant_id,
                event.author_node_id,
                origin_content_item_id,
            );
            let Some(local_content_item_id) = store.federated_content_item_ids.get(&key).copied()
            else {
                return Ok(());
            };
            let local_submission_id = Uuid::from(local_content_item_id);
            if let Some(pod_id) = store
                .pods
                .values()
                .find(|pod| {
                    pod.slug == event.pod_slug
                        && pod.tenant_id == ctx.tenant_id
                        && pod.origin_node_id == Some(event.author_node_id)
                })
                .map(|pod| pod.id)
            {
                let existing = store
                    .accepted_placement_projections
                    .get(&(local_content_item_id, pod_id))
                    .ok_or_else(|| {
                        StoreError::Validation(
                            "Placement Tombstone has no matching accepted Origin Placement".into(),
                        )
                    })?;
                let mut expected = tombstone.origin_placement.clone();
                expected.content_item_id = local_content_item_id;
                expected.pod_id = pod_id;
                if existing != &expected {
                    return Err(StoreError::Validation(
                        "Placement Tombstone does not match accepted Origin Placement evidence"
                            .into(),
                    )
                    .into());
                }
                store.submission_pods.retain(|link| {
                    !(link.pod_id == pod_id && link.submission_id == local_submission_id)
                });
                store
                    .accepted_placement_projections
                    .remove(&(local_content_item_id, pod_id));
                tombstone.origin_placement = expected;
                tombstone.content_reference.content_item_id = local_content_item_id;
                let tombstoned_origin_id = origin_placement_identity(&tombstone.origin_placement);
                for placement in store.pod_placements.values_mut().filter(|placement| {
                    placement.content_item_id == Some(local_content_item_id)
                        && placement
                            .origin_placements
                            .iter()
                            .map(origin_placement_identity)
                            .collect::<HashSet<_>>()
                            .contains(&tombstoned_origin_id)
                }) {
                    placement.origin_withdrawals.push(tombstone.clone());
                }
                store.placement_tombstones.push(tombstone);
            }
        }
        FederatedPodEventType::LegacyLinkRemoved => {}
    }
    Ok(())
}

pub(crate) fn synchronized_origin_pod_id(
    store: &InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
) -> Result<PodId, AgentToolsError> {
    store
        .pods
        .values()
        .find(|pod| {
            pod.slug == event.pod_slug
                && pod.tenant_id == ctx.tenant_id
                && pod.origin_node_id == Some(event.author_node_id)
        })
        .map(|pod| pod.id)
        .ok_or_else(|| StoreError::NotFound("synchronized public Pod".into()).into())
}

pub(crate) fn imported_event_payload<T: serde::de::DeserializeOwned>(
    event: &EventLog,
    field: &str,
) -> Result<T, AgentToolsError> {
    let value = event.payload_json.get(field).cloned().ok_or_else(|| {
        StoreError::Validation(format!(
            "signed {} event is missing {field}",
            event.event_type
        ))
    })?;
    serde_json::from_value(value).map_err(|error| {
        StoreError::Validation(format!(
            "signed {} event has invalid {field}: {error}",
            event.event_type
        ))
        .into()
    })
}

pub(crate) fn imported_event_body<T: serde::de::DeserializeOwned>(
    event: &EventLog,
) -> Result<T, AgentToolsError> {
    serde_json::from_value(event.payload_json.clone()).map_err(|error| {
        StoreError::Validation(format!(
            "signed {} payload is malformed: {error}",
            event.event_type
        ))
        .into()
    })
}

pub(crate) fn project_imported_package(
    store: &mut InMemoryStore,
    local_pod_id: PodId,
    package: &mut PodPackage,
) -> Result<(), AgentToolsError> {
    package.pod_id = local_pod_id;
    package.owner_id = None;
    package.proposer_harness_id = None;
    let version = PackageVersion::new(package.version)
        .map_err(|error| StoreError::Validation(error.to_string()))?;
    if !validate_skill_pack(package).valid {
        return Err(StoreError::Validation("signed Pod Package is invalid".to_string()).into());
    }
    let package_value = normalized_package_value(package, local_pod_id)?;
    if let Some(current) = store.pod_skill_packs.get(&local_pod_id) {
        if package.version < current.version {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot move backwards".to_string(),
            )
            .into());
        }
        if package.version == current.version
            && normalized_package_value(current, local_pod_id)? != package_value
        {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot be reused with different contents".to_string(),
            )
            .into());
        }
    }
    if let Some(existing) = store.pod_package_versions.get(&(local_pod_id, version)) {
        if normalized_package_value(existing, local_pod_id)? != package_value {
            return Err(StoreError::Validation(
                "signed Pod Package history is immutable".to_string(),
            )
            .into());
        }
    }
    store
        .pod_package_versions
        .entry((local_pod_id, version))
        .or_insert_with(|| package.clone());
    store.pod_skill_packs.insert(local_pod_id, package.clone());
    Ok(())
}

pub(crate) fn project_imported_pod(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    origin_node_id: NodeIdentityId,
    mut pod: Pod,
) -> Result<PodId, AgentToolsError> {
    pod.tenant_id = ctx.tenant_id;
    pod.visibility = Visibility::Public;
    pod.created_by = None;
    pod.origin_node_id = Some(origin_node_id);

    if let Some(existing) = store
        .pods
        .values()
        .find(|existing| {
            existing.slug == pod.slug
                && existing.tenant_id == ctx.tenant_id
                && existing.origin_node_id == Some(origin_node_id)
        })
        .cloned()
    {
        ensure_projected_pod_support(store, &existing);
        return Ok(existing.id);
    }

    if store
        .pods
        .values()
        .any(|existing| existing.slug == pod.slug && existing.tenant_id == ctx.tenant_id)
    {
        return Err(StoreError::Duplicate(format!("Pod slug {}", pod.slug)).into());
    }
    let pod_id = Uuid::now_v7();
    pod.id = pod_id;
    store.pods.insert(pod_id, pod.clone());
    ensure_projected_pod_support(store, &pod);
    Ok(pod_id)
}

pub(crate) fn project_imported_submission(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
    mut submission: Submission,
) -> Result<ContentItemId, AgentToolsError> {
    let origin_content_item_id = ContentItemId::from(submission.id);
    let pod_id = store
        .pods
        .values()
        .find(|pod| {
            pod.slug == event.pod_slug
                && pod.tenant_id == ctx.tenant_id
                && pod.origin_node_id == Some(event.author_node_id)
        })
        .map(|pod| pod.id)
        .map(Ok)
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
        })?;

    submission.tenant_id = ctx.tenant_id;
    submission.submitted_by = None;
    submission.origin_event_id = Some(event.event_id);
    let submission_id = store
        .submissions
        .values()
        .find(|existing| {
            existing.tenant_id == ctx.tenant_id
                && existing.canonical_url == submission.canonical_url
        })
        .map(|existing| existing.id)
        .unwrap_or_else(|| {
            let id = Uuid::now_v7();
            submission.id = id;
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
    let local_content_item_id = ContentItemId::from(submission_id);
    store.federated_content_item_ids.insert(
        FederatedContentItemKey::new(ctx.tenant_id, event.author_node_id, origin_content_item_id),
        local_content_item_id,
    );
    Ok(local_content_item_id)
}

pub(crate) fn validate_protocol_version(value: &str) -> Result<(), AgentToolsError> {
    if value == CURRENT_PROTOCOL_VERSION {
        return Ok(());
    }
    Err(AgentToolsError::IncompatibleProtocol {
        received: value.to_string(),
        supported: CURRENT_PROTOCOL_VERSION,
    })
}

pub(crate) fn validate_public_base_url(value: &str, field: &str) -> Result<Url, AgentToolsError> {
    let mut url = parse_public_url(value, field)?;
    url.set_query(None);
    url.set_fragment(None);
    validate_public_scheme_and_host(&url, field)?;
    Ok(url)
}

pub(crate) fn apply_trust_policy_change(
    policy: &mut TrustPolicy,
    change: &TrustPolicyChange,
) -> Result<(), AgentToolsError> {
    match change {
        TrustPolicyChange::AddIndexNode { label, base_url } => {
            let label = label.trim();
            if label.is_empty() {
                return Err(
                    StoreError::Validation("Index Node label must not be empty".into()).into(),
                );
            }
            let base_url = normalized_url(validate_public_base_url(base_url, "base_url")?);
            if !policy
                .index_nodes
                .iter()
                .any(|node| node.base_url == base_url)
            {
                policy.index_nodes.push(IndexNode {
                    label: label.to_string(),
                    base_url,
                });
                policy
                    .index_nodes
                    .sort_by(|left, right| left.base_url.cmp(&right.base_url));
            }
        }
        TrustPolicyChange::RemoveIndexNode { base_url } => {
            let base_url = normalized_url(validate_public_base_url(base_url, "base_url")?);
            let original_len = policy.index_nodes.len();
            policy
                .index_nodes
                .retain(|index| index.base_url != base_url);
            if policy.index_nodes.len() == original_len {
                return Err(StoreError::NotFound(format!("Index Node {base_url}")).into());
            }
        }
        TrustPolicyChange::BlockPod {
            origin_node_id,
            pod_slug,
        } => {
            let pod_slug = pod_slug.trim().to_lowercase();
            if pod_slug.is_empty() {
                return Err(
                    StoreError::Validation("blocked Pod slug must not be empty".into()).into(),
                );
            }
            policy
                .blocked_pods
                .insert(BlockedPod::new(*origin_node_id, pod_slug));
        }
        TrustPolicyChange::BlockNode { node_id } => {
            policy.blocked_nodes.insert(*node_id);
        }
        TrustPolicyChange::BlockSource { source } => {
            insert_normalized_policy_term(&mut policy.blocked_sources, source, "blocked source")?;
        }
        TrustPolicyChange::BlockTopic { topic } => {
            insert_normalized_policy_term(&mut policy.blocked_topics, topic, "blocked topic")?;
        }
    }
    Ok(())
}

