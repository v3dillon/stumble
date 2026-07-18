use clap::{Arg, ArgMatches, Command, ValueHint};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::ExitCode,
};
use stumble_cli::{
    owner_credential_store, paginate, read_json_input, render_text, resolve_existing_data_dir,
    resolve_initialized_data_dir, selected_data_dir, CursorPage, ErrorBody, ErrorEnvelope,
    ExitStatusCategory, OwnerCredentialStore, ResourceDetail, SuccessEnvelope,
};
use stumble_core::{
    new_plaintext_api_token, pod_package_contents_from_files, seed_store,
    validate_pod_package_contents, validate_portable_package_files, AddContentItemToPodRequest,
    AgentHarnessId, AgentHarnessKind, AgentTools, AgentToolsError, AuthContext,
    CandidateConfidence, CandidateId, CandidateReviewState, CandidateSubmissionRequest,
    ContentItemId, CreatePodLifecycleRequest, CreatePodOutcome, CreatePodRequest, CurationPolicy,
    CurationRationale, DiscoveryLeaseSeconds, DiscoveryTask, DiscoveryTaskId, DiscoveryTaskState,
    ExploreRequest, FeedBatch, FeedBatchRequest, FeedbackKind, HarnessCapability, PackageVersion,
    PendingProposalId, PlacementReviewDecision, Pod, PodCreationPackage, PodPackageRevisionOutcome,
    PodRole, RegisterAgentHarnessRequest, ResetLearnedTasteRequest, RouteCandidatePlacementRequest,
    SensitiveChange, StoreError, TasteProfile, UpdateTasteProfileRequest, Visibility,
    PORTABLE_PACKAGE_FILES,
};

fn main() -> ExitCode {
    let matches = match cli().try_get_matches() {
        Ok(matches) => matches,
        Err(error) if error.use_stderr() => {
            return fail(
                ErrorBody::new("usage_error", error.to_string()),
                ExitStatusCategory::Usage,
            );
        }
        Err(error) => {
            print!("{error}");
            return ExitCode::SUCCESS;
        }
    };

    let format = matches
        .get_one::<String>("format")
        .map(String::as_str)
        .unwrap_or("json");
    let data_dir =
        match selected_data_dir(matches.get_one::<PathBuf>("data-dir").map(PathBuf::as_path)) {
            Ok(data_dir) => data_dir,
            Err(error) => return fail(error, ExitStatusCategory::Internal),
        };
    let credentials = owner_credential_store();
    match dispatch(&matches, &data_dir, credentials.as_ref()) {
        Ok(data) => succeed(data, format),
        Err((error, category)) => fail(error, category),
    }
}

fn dispatch(
    matches: &ArgMatches,
    selected_data_dir: &std::path::Path,
    credentials: &dyn OwnerCredentialStore,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let (path, leaf) = command_path(matches);
    if path == "node init" {
        return initialize_node(selected_data_dir, credentials);
    }

    let data_dir = resolve_existing_data_dir(selected_data_dir)
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
    if !AgentTools::home_node_is_initialized(&data_dir).map_err(internal_error)? {
        return Err((
            ErrorBody::new(
                "node_not_initialized",
                format!("Home Node is not initialized at {}", data_dir.display()),
            ),
            ExitStatusCategory::ValidationOrConflict,
        ));
    }
    let tools = AgentTools::open_initialized_home_node(&data_dir).map_err(agent_tools_error)?;
    let actor = authenticate_actor(&tools, &data_dir, credentials)?;

    if path == "node show" {
        let node = tools.node_info(&actor).map_err(agent_tools_error)?;
        return Ok(json!({ "data_dir": data_dir, "node": node, "allowed_actions": [] }));
    }
    match path.as_str() {
        "pod list" => {
            let mut pods = tools
                .list_pods_for_harness(&actor)
                .map_err(agent_tools_error)?;
            pods.sort_by(|left, right| {
                left.slug
                    .cmp(&right.slug)
                    .then_with(|| left.id.cmp(&right.id))
            });
            let items = pods
                .into_iter()
                .map(pod_result)
                .collect::<Result<Vec<_>, _>>()?;
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "pod show" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let allowed_actions = tools
                .pod_allowed_actions(&actor, pod.id)
                .map_err(agent_tools_error)?;
            let mut result = pod_result(pod)?;
            result["allowed_actions"] =
                serde_json::to_value(allowed_actions).map_err(internal_error)?;
            return Ok(result);
        }
        "pod create" => {
            let visibility = leaf
                .get_one::<Visibility>("visibility")
                .expect("required by clap")
                .clone();
            let package = if let Some(path) = leaf.get_one::<PathBuf>("package") {
                let files = read_portable_package_directory(path)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
                PodCreationPackage::Initial {
                    package: pod_package_contents_from_files(&files)
                        .map_err(agent_tools_error_from_store)?,
                }
            } else if let Some(reference) = leaf.get_one::<String>("from-pod") {
                let source = resolve_pod(&tools, &actor, reference)?;
                PodCreationPackage::Derived {
                    source_package: tools
                        .get_skill_pack(&actor, &source.slug)
                        .map_err(agent_tools_error)?,
                }
            } else {
                PodCreationPackage::Default
            };
            let outcome = tools
                .request_create_pod_lifecycle(
                    &actor,
                    CreatePodLifecycleRequest {
                        pod: CreatePodRequest {
                            name: required_string(leaf, "name")?.to_string(),
                            slug: required_string(leaf, "slug")?.to_string(),
                            description: leaf
                                .get_one::<String>("description")
                                .cloned()
                                .unwrap_or_default(),
                            visibility,
                        },
                        package,
                    },
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            return match outcome {
                CreatePodOutcome::Created(pod) => Ok(json!({
                    "status": "created",
                    "result": pod_result(pod)?,
                })),
                CreatePodOutcome::PendingApproval(proposal) => Ok(json!({
                    "status": "pending_approval",
                    "result": proposal,
                })),
            };
        }
        "pod visibility set" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let visibility = leaf
                .get_one::<Visibility>("visibility")
                .expect("required by clap")
                .clone();
            let outcome = tools
                .request_set_pod_visibility(&actor, pod.id, visibility, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "outcome": outcome }));
        }
        "pod explore" => {
            let limit = *leaf.get_one::<u16>("limit").expect("defaulted by clap");
            let request = ExploreRequest::new(
                leaf.get_one::<String>("query").cloned().unwrap_or_default(),
                50,
                *leaf
                    .get_one::<u8>("sample-size")
                    .expect("defaulted by clap") as usize,
            )
            .map_err(|error| agent_tools_error(error.into()))?;
            let explored = tools
                .explore_public_pods(&actor, request)
                .map_err(agent_tools_error)?;
            let cursor = leaf.get_one::<String>("cursor").map(String::as_str);
            let results = paginate(explored.results, limit, cursor)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            return Ok(json!({
                "query": explored.query,
                "items": results.items,
                "next_cursor": results.next_cursor,
            }));
        }
        "pod subscribe" => {
            let reference = required_string(leaf, "pod")?;
            if reference.starts_with("https://") || reference.starts_with("http://") {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(internal_error)?;
                let result = runtime
                    .block_on(stumble_sync::subscribe_pod_from_url(
                        &tools, &actor, reference,
                    ))
                    .map_err(direct_subscription_error)?;
                return Ok(json!({
                    "pod_id": result.subscription.local_pod_id,
                    "slug": result.subscription.pod_slug,
                    "subscription": result.subscription,
                    "imported_events": result.imported_events,
                }));
            }
            let pod = resolve_pod(&tools, &actor, reference)?;
            let subscription = tools
                .subscribe_local_pod(&actor, pod.id)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "subscription": subscription,
            }));
        }
        "pod unsubscribe" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let subscription = tools
                .unsubscribe_pod(&actor, pod.id)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "subscription_id": subscription.id,
                "unsubscribed": true,
            }));
        }
        "pod subscription set" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let is_priority = *leaf.get_one::<bool>("priority").expect("required by clap");
            tools
                .set_priority_subscription(&actor, pod.id, is_priority)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "is_priority": is_priority,
            }));
        }
        "pod role list" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let roles = tools
                .list_pod_roles(&actor, pod.id)
                .map_err(agent_tools_error)?;
            let page = page(roles, leaf)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "items": page.items,
                "next_cursor": page.next_cursor,
            }));
        }
        "pod role grant" | "pod role revoke" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let user_id = required_id::<uuid::Uuid>(leaf, "user-id")?;
            let role = leaf
                .get_one::<PodRole>("role")
                .expect("required by clap")
                .clone();
            let proposal = if path.ends_with(" grant") {
                tools.request_grant_pod_role(&actor, pod.id, user_id, role, chrono::Utc::now())
            } else {
                tools.request_revoke_pod_role(&actor, pod.id, user_id, role, chrono::Utc::now())
            }
            .map_err(agent_tools_error)?;
            return Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "proposal": proposal }));
        }
        "pod content list" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let items = tools
                .pod_content_stream(&actor, pod.id)
                .map_err(agent_tools_error)?;
            let page = page(items, leaf)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "items": page.items,
                "next_cursor": page.next_cursor,
            }));
        }
        "pod content show" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let content_item_id = required_id::<ContentItemId>(leaf, "content-item-id")?;
            let item = tools
                .pod_content_stream(&actor, pod.id)
                .map_err(agent_tools_error)?
                .into_iter()
                .find(|item| item.content_item.id() == content_item_id)
                .ok_or_else(|| {
                    (
                        ErrorBody::new("not_found", "Accepted Pod Content Item was not found"),
                        ExitStatusCategory::ValidationOrConflict,
                    )
                })?;
            let allowed_actions = match tools.pod_curation_policy(&actor, pod.id) {
                Ok(_) => vec!["remove"],
                Err(AgentToolsError::Forbidden { .. }) => Vec::new(),
                Err(error) => return Err(agent_tools_error(error)),
            };
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "content_item": item.content_item,
                "accepted_placement": item.accepted_placement,
                "allowed_actions": allowed_actions,
            }));
        }
        "pod content add" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let content_item_id = required_id::<ContentItemId>(leaf, "content-item-id")?;
            let request = AddContentItemToPodRequest::new(
                content_item_id,
                pod.id,
                leaf.get_one::<String>("note").cloned(),
            )
            .map_err(|error| agent_tools_error(StoreError::Validation(error.to_string()).into()))?;
            let placement = tools
                .add_content_item_to_pod(&actor, request, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "placement": placement,
            }));
        }
        "pod content remove" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let content_item_id = required_id::<ContentItemId>(leaf, "content-item-id")?;
            let reason =
                CurationRationale::new(required_string(leaf, "reason")?).map_err(|error| {
                    agent_tools_error(StoreError::Validation(error.to_string()).into())
                })?;
            let outcome = tools
                .request_remove_content_item_from_pod(
                    &actor,
                    pod.id,
                    content_item_id,
                    reason,
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            let mut result = serde_json::to_value(outcome).map_err(internal_error)?;
            result["pod_id"] = json!(pod.id);
            result["slug"] = json!(pod.slug);
            return Ok(result);
        }
        "pod policy show" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let policy = tools
                .pod_curation_policy(&actor, pod.id)
                .map_err(agent_tools_error)?;
            return Ok(
                json!({ "pod_id": pod.id, "slug": pod.slug, "policy": policy, "allowed_actions": ["set"] }),
            );
        }
        "pod policy set" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let mode = required_string(leaf, "mode")?;
            let threshold = || -> Result<CandidateConfidence, (ErrorBody, ExitStatusCategory)> {
                let value = *leaf
                    .get_one::<f32>("confidence-threshold")
                    .expect("required by clap for threshold policies");
                CandidateConfidence::new(value).map_err(|error| {
                    agent_tools_error(StoreError::Validation(error.to_string()).into())
                })
            };
            if mode == "autonomous" {
                let proposal = tools
                    .create_pending_proposal(
                        &actor,
                        SensitiveChange::EnableAutonomousCuration {
                            pod_id: pod.id,
                            confidence_threshold: threshold()?,
                        },
                        chrono::Utc::now(),
                        chrono::Utc::now() + chrono::Duration::hours(24),
                    )
                    .map_err(agent_tools_error)?;
                return Ok(json!({
                    "pod_id": pod.id,
                    "slug": pod.slug,
                    "status": "pending_approval",
                    "proposal": proposal,
                }));
            }
            let policy = if mode == "manual" {
                CurationPolicy::Manual
            } else {
                CurationPolicy::Assisted {
                    confidence_threshold: threshold()?,
                }
            };
            let policy = tools
                .set_pod_curation_policy(&actor, pod.id, policy, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "status": "updated",
                "policy": policy,
            }));
        }
        "pod package show" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let current = tools
                .get_skill_pack(&actor, &pod.slug)
                .map_err(agent_tools_error)?;
            let requested = leaf
                .get_one::<i32>("version")
                .copied()
                .map(PackageVersion::new)
                .transpose()
                .map_err(|error| {
                    agent_tools_error(StoreError::Validation(error.to_string()).into())
                })?;
            let package = if let Some(version) = requested {
                tools
                    .get_pod_package_version(&actor, &pod.slug, version)
                    .map_err(agent_tools_error)?
            } else {
                current.clone()
            };
            let mut allowed_actions = Vec::new();
            if package.version == current.version {
                allowed_actions.push("export");
                if tools
                    .require_harness_capability(&actor, HarnessCapability::PackageManagement)
                    .is_ok()
                {
                    allowed_actions.push("revise");
                }
            }
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "package": package,
                "allowed_actions": allowed_actions,
            }));
        }
        "pod package export" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let output = leaf.get_one::<PathBuf>("output").expect("required by clap");
            let export = tools
                .export_skill_pack(&actor, &pod.slug)
                .map_err(agent_tools_error)?;
            std::fs::create_dir_all(output).map_err(|error| {
                (
                    ErrorBody::new("package_export_failed", error.to_string()),
                    ExitStatusCategory::Internal,
                )
            })?;
            for (name, contents) in export.files {
                std::fs::write(output.join(name), contents).map_err(|error| {
                    (
                        ErrorBody::new("package_export_failed", error.to_string()),
                        ExitStatusCategory::Internal,
                    )
                })?;
            }
            let package = tools
                .get_skill_pack(&actor, &pod.slug)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "version": package.version,
                "output": output,
            }));
        }
        "pod package validate" => {
            let directory = leaf
                .get_one::<PathBuf>("package")
                .expect("required by clap");
            let files = read_portable_package_directory(directory)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            validate_portable_package_files(&files).map_err(agent_tools_error_from_store)?;
            let contents =
                pod_package_contents_from_files(&files).map_err(agent_tools_error_from_store)?;
            let report = validate_pod_package_contents(&contents);
            return serde_json::to_value(report).map_err(internal_error);
        }
        "pod package revise" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let directory = leaf
                .get_one::<PathBuf>("package")
                .expect("required by clap");
            let files = read_portable_package_directory(directory)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            let base_version = PackageVersion::new(
                *leaf
                    .get_one::<i32>("base-version")
                    .expect("required by clap"),
            )
            .map_err(|error| agent_tools_error(StoreError::Validation(error.to_string()).into()))?;
            let outcome = tools
                .request_revise_pod_package(&actor, pod.id, base_version, files, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return Ok(match outcome {
                PodPackageRevisionOutcome::Revised(package) => json!({
                    "pod_id": pod.id,
                    "slug": pod.slug,
                    "status": "revised",
                    "package": package,
                }),
                PodPackageRevisionOutcome::PendingApproval(proposal) => json!({
                    "pod_id": pod.id,
                    "slug": pod.slug,
                    "status": "pending_approval",
                    "proposal": proposal,
                }),
            });
        }
        "discover task list" => {
            let now = chrono::Utc::now();
            let pod = leaf
                .get_one::<String>("pod")
                .map(|reference| resolve_pod(&tools, &actor, reference))
                .transpose()?;
            let state = leaf.get_one::<String>("state").map(String::as_str);
            let mut items = tools
                .list_discovery_tasks(&actor, now)
                .map_err(agent_tools_error)?;
            items.retain(|task| {
                pod.as_ref().is_none_or(|pod| task.pod_id == pod.id)
                    && discovery_task_matches_state(task, state, now)
            });
            items.sort_by(|left, right| {
                left.due_at
                    .cmp(&right.due_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "discover task show" => {
            let id = required_id::<DiscoveryTaskId>(leaf, "id")?;
            let now = chrono::Utc::now();
            let task = tools
                .discovery_task_status(&actor, id, now)
                .map_err(agent_tools_error)?;
            return discovery_task_detail(&actor, task, now);
        }
        "discover task claim" => {
            let id = required_id::<DiscoveryTaskId>(leaf, "id")?;
            let lease = discovery_lease(leaf)?;
            let now = chrono::Utc::now();
            let task = tools
                .claim_discovery_task(&actor, id, now, lease)
                .map_err(agent_tools_error)?;
            return discovery_task_mutation_result(&actor, task, now);
        }
        "discover task renew" => {
            let id = required_id::<DiscoveryTaskId>(leaf, "id")?;
            let lease = discovery_lease(leaf)?;
            let now = chrono::Utc::now();
            let task = tools
                .renew_discovery_task_lease(&actor, id, now, lease)
                .map_err(agent_tools_error)?;
            return discovery_task_mutation_result(&actor, task, now);
        }
        "discover task complete" => {
            let id = required_id::<DiscoveryTaskId>(leaf, "id")?;
            let now = chrono::Utc::now();
            let task = tools
                .complete_discovery_task(&actor, id, now)
                .map_err(agent_tools_error)?;
            return discovery_task_mutation_result(&actor, task, now);
        }
        "discover task fail" => {
            let id = required_id::<DiscoveryTaskId>(leaf, "id")?;
            let now = chrono::Utc::now();
            let task = tools
                .fail_discovery_task(&actor, id, now, required_string(leaf, "reason")?.to_owned())
                .map_err(agent_tools_error)?;
            return discovery_task_mutation_result(&actor, task, now);
        }
        "discover candidate list" => {
            let status = leaf.get_one::<String>("status").map(String::as_str);
            let mut items = tools.list_candidates(&actor).map_err(agent_tools_error)?;
            items.retain(|candidate| match status {
                None => true,
                Some("pending") => candidate.review_state == CandidateReviewState::Pending,
                Some("accepted") => candidate.review_state == CandidateReviewState::Accepted,
                Some(_) => false,
            });
            items.sort_by_key(|candidate| (candidate.created_at, candidate.id));
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "discover candidate submit" => {
            let input = leaf.get_one::<PathBuf>("input").expect("required by clap");
            let idempotency_key = required_string(leaf, "idempotency-key")?;
            let mut input = read_json_input(input)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            let input_object = input.as_object_mut().ok_or_else(|| {
                (
                    ErrorBody::new("invalid_input", "Candidate input must be a JSON object"),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            input_object.insert("harness_idempotency_key".into(), json!(idempotency_key));
            input_object.insert("client_idempotency_key".into(), json!(idempotency_key));
            let request: CandidateSubmissionRequest =
                serde_json::from_value(input).map_err(|error| {
                    (
                        ErrorBody::new("invalid_input", error.to_string()),
                        ExitStatusCategory::ValidationOrConflict,
                    )
                })?;
            return serde_json::to_value(
                tools
                    .submit_candidate(&actor, request)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error);
        }
        "discover candidate show" => {
            let candidate_id = required_id::<CandidateId>(leaf, "candidate-id")?;
            let inspection = tools
                .inspect_candidate(&actor, candidate_id)
                .map_err(agent_tools_error)?;
            return candidate_inspection_result(&tools, &actor, inspection);
        }
        "discover candidate evaluate" => {
            let candidate_id = required_id::<CandidateId>(leaf, "candidate-id")?;
            let result = tools
                .curate_candidate(&actor, candidate_id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            let placements = pod_placement_results(&tools, &actor, result.placements)?;
            return Ok(json!({
                "candidate": result.candidate,
                "content_item": result.content_item,
                "placements": placements,
            }));
        }
        "discover candidate route" => {
            let candidate_id = required_id::<CandidateId>(leaf, "candidate-id")?;
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let confidence = CandidateConfidence::new(
                *leaf.get_one::<f32>("confidence").expect("required by clap"),
            )
            .map_err(|error| agent_tools_error(StoreError::Validation(error.to_string()).into()))?;
            let request = RouteCandidatePlacementRequest::new(
                pod.id,
                required_string(leaf, "reason")?,
                confidence,
            )
            .map_err(|error| agent_tools_error(error.into()))?;
            let placement = tools
                .route_candidate_placement(&actor, candidate_id, request, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "placement": placement }));
        }
        "discover candidate review" => {
            let candidate_id = required_id::<CandidateId>(leaf, "candidate-id")?;
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let decision = match required_string(leaf, "decision")? {
                "accept" => PlacementReviewDecision::Accept,
                "reject" => PlacementReviewDecision::Reject,
                _ => unreachable!("constrained by clap"),
            };
            let note = leaf
                .get_one::<String>("note")
                .cloned()
                .map(CurationRationale::new)
                .transpose()
                .map_err(|error| agent_tools_error(error.into()))?;
            let placement = tools
                .review_candidate_placement(
                    &actor,
                    candidate_id,
                    pod.id,
                    decision,
                    note,
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            return Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "placement": placement }));
        }
        "feed batch get" => {
            let request = if let Some(input) = leaf.get_one::<PathBuf>("input") {
                let value = read_json_input(input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
                serde_json::from_value::<FeedBatchRequest>(value).map_err(json_input_error)?
            } else {
                FeedBatchRequest::new(7).expect("the default Feed Batch size is valid")
            };
            let batch = tools
                .get_feed_batch(&actor, request, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return feed_batch_result(&tools, &actor, batch);
        }
        "feed batch complete" => {
            let id = required_id::<uuid::Uuid>(leaf, "id")?;
            let batch = tools
                .complete_feed_batch(&actor, id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return feed_batch_result(&tools, &actor, batch);
        }
        "feed feedback record" => {
            let content_item_id = required_id::<ContentItemId>(leaf, "content-item-id")?;
            let kind = *leaf
                .get_one::<FeedbackKind>("kind")
                .expect("required by clap");
            let feedback_state = tools
                .record_feed_feedback(
                    &actor,
                    content_item_id,
                    kind,
                    leaf.get_one::<String>("topic").cloned(),
                    leaf.get_one::<String>("reason").cloned(),
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "content_item_id": content_item_id,
                "feedback_state": feedback_state,
                "allowed_actions": [
                    "save",
                    "more_like_this",
                    "less_like_this",
                    "dismiss",
                    "block_source",
                    "block_topic",
                ],
            }));
        }
        "feed taste show" => {
            let profile = tools.taste_profile(&actor).map_err(agent_tools_error)?;
            return taste_profile_result(profile);
        }
        "feed taste set" => {
            let input = leaf.get_one::<PathBuf>("input").expect("required by clap");
            let value = read_json_input(input)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            let request = serde_json::from_value::<UpdateTasteProfileRequest>(value)
                .map_err(json_input_error)?;
            let profile = tools
                .update_taste_profile(&actor, request)
                .map_err(agent_tools_error)?;
            return taste_profile_result(profile);
        }
        "feed taste reset" => {
            let request = if let Some(input) = leaf.get_one::<PathBuf>("input") {
                let value = read_json_input(input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
                serde_json::from_value::<ResetLearnedTasteRequest>(value)
                    .map_err(json_input_error)?
            } else {
                ResetLearnedTasteRequest::all()
            };
            let profile = tools
                .reset_learned_taste(&actor, request)
                .map_err(agent_tools_error)?;
            return taste_profile_result(profile);
        }
        "node harness list" => {
            let items = tools
                .list_agent_harnesses(&actor)
                .map_err(agent_tools_error)?;
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "node harness show" => {
            let id = required_id::<AgentHarnessId>(leaf, "id")?;
            let view = tools.agent_harness(&actor, id).map_err(agent_tools_error)?;
            let allowed_actions = if actor.harness_id.is_none()
                && view.status == stumble_core::AgentHarnessStatus::Active
            {
                vec!["revoke"]
            } else {
                Vec::new()
            };
            return serde_json::to_value(ResourceDetail {
                resource: view,
                allowed_actions,
            })
            .map_err(internal_error);
        }
        "node harness register" => {
            if actor.harness_id.is_some() {
                return Err((
                    ErrorBody::new(
                        "forbidden",
                        "only the Home Node Owner may register an Agent Harness directly",
                    ),
                    ExitStatusCategory::Authorization,
                ));
            }
            let request = RegisterAgentHarnessRequest {
                label: leaf
                    .get_one::<String>("label")
                    .cloned()
                    .expect("required by clap"),
                kind: *leaf
                    .get_one::<AgentHarnessKind>("kind")
                    .expect("required by clap"),
                capabilities: leaf
                    .get_many::<HarnessCapability>("capability")
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect(),
                pod_ids: leaf
                    .get_many::<stumble_core::PodId>("pod-id")
                    .map(|values| values.copied().collect()),
            };
            let issued = tools
                .register_agent_harness(&actor, request)
                .map_err(agent_tools_error)?;
            return Ok(json!({ "harness": issued.harness, "credential": issued.token.expose() }));
        }
        "node harness revoke" => {
            if actor.harness_id.is_some() {
                return Err((
                    ErrorBody::new(
                        "forbidden",
                        "only the Home Node Owner may revoke an Agent Harness directly",
                    ),
                    ExitStatusCategory::Authorization,
                ));
            }
            let id = required_id::<AgentHarnessId>(leaf, "id")?;
            tools
                .revoke_agent_harness(&actor, id)
                .map_err(agent_tools_error)?;
            return Ok(json!({ "id": id, "status": "revoked" }));
        }
        "node proposal list" => {
            let items = tools
                .list_pending_proposals(&actor, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "node proposal show" => {
            let id = required_id::<PendingProposalId>(leaf, "id")?;
            let proposal = tools
                .pending_proposal(&actor, id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            let allowed_actions = tools
                .pending_proposal_allowed_actions(&actor, id)
                .map_err(agent_tools_error)?;
            return serde_json::to_value(ResourceDetail {
                resource: proposal,
                allowed_actions,
            })
            .map_err(internal_error);
        }
        "node proposal approve" => {
            let id = required_id::<PendingProposalId>(leaf, "id")?;
            return serde_json::to_value(
                tools
                    .approve_pending_proposal(&actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error);
        }
        "node proposal reject" => {
            let id = required_id::<PendingProposalId>(leaf, "id")?;
            let reason = leaf
                .get_one::<String>("reason")
                .cloned()
                .expect("required by clap");
            return serde_json::to_value(
                tools
                    .reject_pending_proposal(&actor, id, chrono::Utc::now(), reason)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error);
        }
        _ => {}
    }
    if let Some(input) = leaf.try_get_one::<PathBuf>("input").ok().flatten() {
        let input = read_json_input(input)
            .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
        return Ok(json!({
            "command": path,
            "status": "shell_available",
            "input": input
        }));
    }
    if path.ends_with(" list") || path == "pod list" || path == "pod explore" {
        return serde_json::to_value(CursorPage::<Value>::empty()).map_err(|error| {
            (
                ErrorBody::new("internal_error", error.to_string()),
                ExitStatusCategory::Internal,
            )
        });
    }
    Ok(json!({
        "command": path,
        "status": "shell_available",
        "allowed_actions": []
    }))
}

fn authenticate_actor(
    tools: &AgentTools,
    data_dir: &std::path::Path,
    credentials: &dyn OwnerCredentialStore,
) -> Result<AuthContext, (ErrorBody, ExitStatusCategory)> {
    if let Ok(credential) = std::env::var("STUMBLE_HARNESS_CREDENTIAL") {
        return tools
            .authenticate_token(&credential)
            .map_err(agent_tools_error)?
            .ok_or_else(|| {
                (
                    ErrorBody::new(
                        "invalid_harness_credential",
                        "Agent Harness credential is invalid or revoked",
                    ),
                    ExitStatusCategory::Authorization,
                )
            });
    }
    credentials
        .load(data_dir)
        .map_err(credential_error)?
        .ok_or_else(|| {
            (
                ErrorBody::new(
                    "owner_credential_not_found",
                    "Home Node Owner credential was not found in the credential store",
                ),
                ExitStatusCategory::Authorization,
            )
        })?;
    tools.local_owner_auth_context().map_err(agent_tools_error)
}

fn required_id<T>(matches: &ArgMatches, name: &str) -> Result<T, (ErrorBody, ExitStatusCategory)>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    matches
        .get_one::<String>(name)
        .expect("required by clap")
        .parse::<T>()
        .map_err(|error: T::Err| {
            (
                ErrorBody::new("invalid_id", error.to_string()),
                ExitStatusCategory::ValidationOrConflict,
            )
        })
}

fn required_string<'a>(
    matches: &'a ArgMatches,
    name: &str,
) -> Result<&'a str, (ErrorBody, ExitStatusCategory)> {
    matches
        .get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| internal_error(format!("missing required argument {name}")))
}

fn resolve_pod(
    tools: &AgentTools,
    actor: &AuthContext,
    reference: &str,
) -> Result<Pod, (ErrorBody, ExitStatusCategory)> {
    tools
        .list_pods_for_harness(actor)
        .map_err(agent_tools_error)?
        .into_iter()
        .find(|pod| pod.slug == reference || pod.id.to_string() == reference)
        .ok_or_else(|| {
            (
                ErrorBody::new("not_found", format!("Pod {reference} was not found")),
                ExitStatusCategory::ValidationOrConflict,
            )
        })
}

fn pod_result(pod: Pod) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let pod_id = pod.id;
    let mut result = serde_json::to_value(pod).map_err(internal_error)?;
    result["pod_id"] = json!(pod_id);
    Ok(result)
}

fn discovery_task_matches_state(
    task: &DiscoveryTask,
    state: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    match state {
        None => true,
        Some("ready") => task.state == DiscoveryTaskState::Pending && task.due_at <= now,
        Some("pending") => task.state == DiscoveryTaskState::Pending,
        Some("leased") => matches!(task.state, DiscoveryTaskState::Leased(_)),
        Some("completed") => task.state == DiscoveryTaskState::Completed,
        Some("terminal-failure") => task.state == DiscoveryTaskState::TerminalFailure,
        Some(_) => false,
    }
}

fn discovery_task_allowed_actions(
    actor: &AuthContext,
    task: &DiscoveryTask,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<&'static str> {
    let Some(harness_id) = actor.harness_id else {
        return Vec::new();
    };
    match &task.state {
        DiscoveryTaskState::Pending if task.due_at <= now => vec!["claim"],
        DiscoveryTaskState::Leased(lease)
            if lease.harness_id == harness_id && lease.expires_at > now =>
        {
            vec!["renew", "complete", "fail"]
        }
        _ => Vec::new(),
    }
}

fn discovery_task_detail(
    actor: &AuthContext,
    task: DiscoveryTask,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let allowed_actions = discovery_task_allowed_actions(actor, &task, now);
    serde_json::to_value(ResourceDetail {
        resource: task,
        allowed_actions,
    })
    .map_err(internal_error)
}

fn discovery_task_mutation_result(
    actor: &AuthContext,
    task: DiscoveryTask,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let allowed_actions = discovery_task_allowed_actions(actor, &task, now);
    Ok(json!({ "task": task, "allowed_actions": allowed_actions }))
}

fn candidate_inspection_result(
    tools: &AgentTools,
    actor: &AuthContext,
    inspection: stumble_core::CandidateInspection,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let placements = pod_placement_results(tools, actor, inspection.placements)?;
    Ok(json!({
        "candidate": inspection.candidate,
        "submissions": inspection.submissions,
        "placements": placements,
        "allowed_actions": inspection.allowed_actions,
    }))
}

fn pod_placement_results(
    tools: &AgentTools,
    actor: &AuthContext,
    placements: Vec<stumble_core::PodPlacement>,
) -> Result<Vec<Value>, (ErrorBody, ExitStatusCategory)> {
    placements
        .into_iter()
        .map(|placement| {
            let pod = resolve_pod(tools, actor, &placement.pod_id.to_string())?;
            let mut value = serde_json::to_value(placement).map_err(internal_error)?;
            value["slug"] = json!(pod.slug);
            Ok(value)
        })
        .collect()
}

fn feed_batch_result(
    tools: &AgentTools,
    actor: &AuthContext,
    batch: FeedBatch,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let can_complete = batch.completed_at.is_none();
    let mut value = serde_json::to_value(batch).map_err(internal_error)?;
    if let Some(items) = value["items"].as_array_mut() {
        for item in items {
            if let Some(placements) = item["placements"].as_array_mut() {
                for placement in placements {
                    let pod_id = placement["pod_id"]
                        .as_str()
                        .ok_or_else(|| internal_error("Feed placement did not contain a Pod ID"))?;
                    let pod = resolve_pod(tools, actor, pod_id)?;
                    placement["slug"] = json!(pod.slug);
                }
            }
        }
    }
    value["allowed_actions"] = if can_complete {
        json!(["complete"])
    } else {
        json!([])
    };
    Ok(value)
}

fn taste_profile_result(profile: TasteProfile) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let mut value = serde_json::to_value(profile).map_err(internal_error)?;
    value["allowed_actions"] = json!(["set", "reset"]);
    Ok(value)
}

fn discovery_lease(
    matches: &ArgMatches,
) -> Result<DiscoveryLeaseSeconds, (ErrorBody, ExitStatusCategory)> {
    let seconds = *matches
        .get_one::<u64>("lease-seconds")
        .expect("required by clap");
    DiscoveryLeaseSeconds::new(seconds).map_err(|error| {
        (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        )
    })
}

fn direct_subscription_error(
    error: stumble_sync::DirectSubscriptionError,
) -> (ErrorBody, ExitStatusCategory) {
    match error {
        stumble_sync::DirectSubscriptionError::Core(error) => agent_tools_error(error),
        stumble_sync::DirectSubscriptionError::InvalidAddress(error) => agent_tools_error(error),
        error => internal_error(error),
    }
}

fn page<T>(
    items: Vec<T>,
    matches: &ArgMatches,
) -> Result<CursorPage<T>, (ErrorBody, ExitStatusCategory)> {
    let limit = *matches.get_one::<u16>("limit").expect("defaulted by clap");
    let cursor = matches.get_one::<String>("cursor").map(String::as_str);
    paginate(items, limit, cursor)
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))
}

fn initialize_node(
    selected_data_dir: &std::path::Path,
    credentials: &dyn OwnerCredentialStore,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let data_dir = resolve_initialized_data_dir(selected_data_dir)
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
    if AgentTools::home_node_is_initialized(&data_dir).map_err(internal_error)? {
        return Err((
            ErrorBody::new(
                "node_already_initialized",
                format!("Home Node is already initialized at {}", data_dir.display()),
            ),
            ExitStatusCategory::ValidationOrConflict,
        ));
    }

    let credential = new_plaintext_api_token();
    credentials
        .store(&data_dir, &credential)
        .map_err(credential_error)?;
    let tools = match AgentTools::initialize_home_node(&data_dir, seed_store) {
        Ok(tools) => tools,
        Err(error) => {
            let _ = credentials.remove(&data_dir);
            return Err(agent_tools_error(error));
        }
    };
    let owner = tools
        .local_owner_auth_context()
        .map_err(agent_tools_error)?;
    let node = tools.node_info(&owner).map_err(agent_tools_error)?;
    Ok(json!({ "data_dir": data_dir, "node": node }))
}

fn credential_error(error: impl std::fmt::Display) -> (ErrorBody, ExitStatusCategory) {
    (
        ErrorBody::new("credential_store_error", error.to_string()),
        ExitStatusCategory::Internal,
    )
}

fn internal_error(error: impl std::fmt::Display) -> (ErrorBody, ExitStatusCategory) {
    (
        ErrorBody::new("internal_error", error.to_string()),
        ExitStatusCategory::Internal,
    )
}

fn json_input_error(error: serde_json::Error) -> (ErrorBody, ExitStatusCategory) {
    (
        ErrorBody::new("validation_error", error.to_string()),
        ExitStatusCategory::ValidationOrConflict,
    )
}

fn agent_tools_error(error: AgentToolsError) -> (ErrorBody, ExitStatusCategory) {
    match error {
        AgentToolsError::NodeNotInitialized => (
            ErrorBody::new("node_not_initialized", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::NodeAlreadyInitialized => (
            ErrorBody::new("node_already_initialized", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Forbidden { .. } => (
            ErrorBody::new("forbidden", error.to_string()),
            ExitStatusCategory::Authorization,
        ),
        AgentToolsError::Store(StoreError::Validation(_)) => (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Store(StoreError::NotFound(_)) => (
            ErrorBody::new("not_found", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Store(StoreError::Duplicate(_)) | AgentToolsError::BadUrl(_) => (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Store(StoreError::InvalidSignature) => (
            ErrorBody::new("invalid_signature", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::TaskLeaseConflict => (
            ErrorBody::new("task_lease_conflict", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::TaskLeaseRequired => (
            ErrorBody::new("task_lease_required", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::TaskTerminal => (
            ErrorBody::new("task_terminal", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::CandidateIdempotencyConflict => (
            ErrorBody::new("idempotency_conflict", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::CandidateHarnessRequired
        | AgentToolsError::CandidateTaskRequired
        | AgentToolsError::CandidateTaskLeaseRequired
        | AgentToolsError::CandidatePackageVersionMismatch => (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        error => internal_error(error),
    }
}

fn agent_tools_error_from_store(error: StoreError) -> (ErrorBody, ExitStatusCategory) {
    agent_tools_error(error.into())
}

fn read_portable_package_directory(path: &Path) -> Result<BTreeMap<String, String>, ErrorBody> {
    let entries = std::fs::read_dir(path).map_err(|error| {
        ErrorBody::new(
            "invalid_package_directory",
            format!("could not read {}: {error}", path.display()),
        )
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|error| ErrorBody::new("invalid_package_directory", error.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !PORTABLE_PACKAGE_FILES.contains(&name.as_str()) {
            return Err(ErrorBody::new(
                "invalid_package_directory",
                format!("unsupported portable Pod Package file {name}"),
            ));
        }
    }
    PORTABLE_PACKAGE_FILES
        .iter()
        .map(|name| {
            std::fs::read_to_string(path.join(name))
                .map(|contents| ((*name).to_string(), contents))
                .map_err(|error| {
                    ErrorBody::new(
                        "invalid_package_directory",
                        format!("could not read {}: {error}", path.join(name).display()),
                    )
                })
        })
        .collect()
}

fn command_path(matches: &ArgMatches) -> (String, &ArgMatches) {
    let mut names = Vec::new();
    let mut current = matches;
    while let Some((name, child)) = current.subcommand() {
        names.push(name);
        current = child;
    }
    (names.join(" "), current)
}

fn succeed(data: Value, format: &str) -> ExitCode {
    if format == "text" {
        print!("{}", render_text(&data));
    } else {
        println!(
            "{}",
            serde_json::to_string(&SuccessEnvelope::new(data))
                .expect("JSON values always serialize")
        );
    }
    ExitCode::SUCCESS
}

fn fail(error: ErrorBody, category: ExitStatusCategory) -> ExitCode {
    eprintln!(
        "{}",
        serde_json::to_string(&ErrorEnvelope::new(error))
            .expect("error envelope always serializes")
    );
    ExitCode::from(category as u8)
}

fn cli() -> Command {
    Command::new("stumble")
        .about("Operate a local Stumble Home Node")
        .arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .default_value("json")
                .value_parser(["json", "text"]),
        )
        .arg(
            Arg::new("data-dir")
                .long("data-dir")
                .global(true)
                .env("STUMBLE_DATA_DIR")
                .value_parser(clap::value_parser!(PathBuf))
                .value_hint(ValueHint::DirPath),
        )
        .subcommand_required(true)
        .subcommand(node())
        .subcommand(pod())
        .subcommand(discover())
        .subcommand(feed())
        .subcommand(sync())
}

fn node() -> Command {
    Command::new("node")
        .subcommand_required(true)
        .subcommand(Command::new("init"))
        .subcommand(Command::new("show"))
        .subcommand(harness())
        .subcommand(proposal())
}

fn harness() -> Command {
    Command::new("harness")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show").arg(Arg::new("id").required(true)))
        .subcommand(
            Command::new("register")
                .arg(Arg::new("label").long("label").required(true))
                .arg(
                    Arg::new("kind")
                        .long("kind")
                        .required(true)
                        .value_parser(clap::value_parser!(AgentHarnessKind)),
                )
                .arg(
                    Arg::new("capability")
                        .long("capability")
                        .required(true)
                        .action(clap::ArgAction::Append)
                        .value_parser(clap::value_parser!(HarnessCapability)),
                )
                .arg(
                    Arg::new("pod-id")
                        .long("pod-id")
                        .action(clap::ArgAction::Append)
                        .value_parser(clap::value_parser!(stumble_core::PodId)),
                ),
        )
        .subcommand(Command::new("revoke").arg(Arg::new("id").required(true)))
}

fn proposal() -> Command {
    Command::new("proposal")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show").arg(Arg::new("id").required(true)))
        .subcommand(Command::new("approve").arg(Arg::new("id").required(true)))
        .subcommand(
            Command::new("reject")
                .arg(Arg::new("id").required(true))
                .arg(Arg::new("reason").long("reason").required(true)),
        )
}

fn pod() -> Command {
    Command::new("pod")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show").arg(Arg::new("pod").required(true)))
        .subcommand(
            Command::new("create")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("slug").long("slug").required(true))
                .arg(Arg::new("description").long("description"))
                .arg(visibility_arg())
                .arg(
                    Arg::new("package")
                        .long("package")
                        .value_parser(clap::value_parser!(PathBuf))
                        .value_hint(ValueHint::DirPath)
                        .conflicts_with("from-pod"),
                )
                .arg(Arg::new("from-pod").long("from-pod")),
        )
        .subcommand(
            list_leaf("explore")
                .arg(Arg::new("query").long("query"))
                .arg(
                    Arg::new("sample-size")
                        .long("sample-size")
                        .default_value("3")
                        .value_parser(clap::value_parser!(u8).range(0..=10)),
                ),
        )
        .subcommand(Command::new("subscribe").arg(Arg::new("pod").required(true)))
        .subcommand(Command::new("unsubscribe").arg(Arg::new("pod").required(true)))
        .subcommand(
            Command::new("subscription")
                .subcommand_required(true)
                .subcommand(
                    Command::new("set").arg(Arg::new("pod").required(true)).arg(
                        Arg::new("priority")
                            .long("priority")
                            .required(true)
                            .value_parser(clap::value_parser!(bool)),
                    ),
                ),
        )
        .subcommand(
            Command::new("visibility")
                .subcommand_required(true)
                .subcommand(
                    Command::new("set")
                        .arg(Arg::new("pod").required(true))
                        .arg(visibility_arg()),
                ),
        )
        .subcommand(
            Command::new("role")
                .subcommand_required(true)
                .subcommand(list_leaf("list").arg(Arg::new("pod").required(true)))
                .subcommand(role_change("grant"))
                .subcommand(role_change("revoke")),
        )
        .subcommand(content())
        .subcommand(policy())
        .subcommand(package())
}

fn visibility_arg() -> Arg {
    Arg::new("visibility")
        .long("visibility")
        .required(true)
        .value_parser(clap::builder::TypedValueParser::map(
            clap::builder::PossibleValuesParser::new(["private", "invite-only", "public"]),
            |value| match value.as_str() {
                "private" => Visibility::Private,
                "invite-only" => Visibility::InviteOnly,
                "public" => Visibility::Public,
                _ => unreachable!("constrained by possible values"),
            },
        ))
}

fn role_change(name: &'static str) -> Command {
    Command::new(name)
        .arg(Arg::new("pod").required(true))
        .arg(Arg::new("user-id").long("user-id").required(true))
        .arg(Arg::new("role").long("role").required(true).value_parser(
            clap::builder::TypedValueParser::map(
                clap::builder::PossibleValuesParser::new(["owner", "curator"]),
                |value| match value.as_str() {
                    "owner" => PodRole::Owner,
                    "curator" => PodRole::Curator,
                    _ => unreachable!("constrained by possible values"),
                },
            ),
        ))
}

fn content() -> Command {
    Command::new("content")
        .subcommand_required(true)
        .subcommand(list_leaf("list").arg(Arg::new("pod").required(true)))
        .subcommand(content_item_command("show"))
        .subcommand(content_item_command("add").arg(Arg::new("note").long("note")))
        .subcommand(
            content_item_command("remove").arg(Arg::new("reason").long("reason").required(true)),
        )
}

fn content_item_command(name: &'static str) -> Command {
    Command::new(name)
        .arg(Arg::new("pod").required(true))
        .arg(Arg::new("content-item-id").required(true))
}

fn policy() -> Command {
    Command::new("policy")
        .subcommand_required(true)
        .subcommand(Command::new("show").arg(Arg::new("pod").required(true)))
        .subcommand(
            Command::new("set")
                .arg(Arg::new("pod").required(true))
                .arg(Arg::new("mode").long("mode").required(true).value_parser([
                    "manual",
                    "assisted",
                    "autonomous",
                ]))
                .arg(
                    Arg::new("confidence-threshold")
                        .long("confidence-threshold")
                        .value_parser(clap::value_parser!(f32))
                        .required_if_eq_any([("mode", "assisted"), ("mode", "autonomous")]),
                ),
        )
}

fn package() -> Command {
    let directory = || {
        Arg::new("package")
            .long("package")
            .required(true)
            .value_parser(clap::value_parser!(PathBuf))
            .value_hint(ValueHint::DirPath)
    };
    Command::new("package")
        .subcommand_required(true)
        .subcommand(
            Command::new("show")
                .arg(Arg::new("pod").required(true))
                .arg(
                    Arg::new("version")
                        .long("version")
                        .value_parser(clap::value_parser!(i32).range(1..)),
                ),
        )
        .subcommand(
            Command::new("export")
                .arg(Arg::new("pod").required(true))
                .arg(
                    Arg::new("output")
                        .long("output")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf))
                        .value_hint(ValueHint::DirPath),
                ),
        )
        .subcommand(Command::new("validate").arg(directory()))
        .subcommand(
            Command::new("revise")
                .arg(Arg::new("pod").required(true))
                .arg(
                    Arg::new("base-version")
                        .long("base-version")
                        .required(true)
                        .value_parser(clap::value_parser!(i32).range(1..)),
                )
                .arg(directory()),
        )
}

fn discover() -> Command {
    Command::new("discover")
        .subcommand_required(true)
        .subcommand(discovery_task())
        .subcommand(candidate())
}

fn candidate() -> Command {
    let candidate_id = || Arg::new("candidate-id").required(true);
    Command::new("candidate")
        .subcommand_required(true)
        .subcommand(
            list_leaf("list").arg(
                Arg::new("status")
                    .long("status")
                    .value_parser(["pending", "accepted"]),
            ),
        )
        .subcommand(
            Command::new("submit")
                .arg(
                    Arg::new("input")
                        .long("input")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf))
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("idempotency-key")
                        .long("idempotency-key")
                        .required(true),
                ),
        )
        .subcommand(Command::new("show").arg(candidate_id()))
        .subcommand(Command::new("evaluate").arg(candidate_id()))
        .subcommand(
            Command::new("route")
                .arg(candidate_id())
                .arg(Arg::new("pod").required(true))
                .arg(Arg::new("reason").long("reason").required(true))
                .arg(
                    Arg::new("confidence")
                        .long("confidence")
                        .required(true)
                        .value_parser(clap::value_parser!(f32)),
                ),
        )
        .subcommand(
            Command::new("review")
                .arg(candidate_id())
                .arg(Arg::new("pod").required(true))
                .arg(
                    Arg::new("decision")
                        .long("decision")
                        .required(true)
                        .value_parser(["accept", "reject"]),
                )
                .arg(Arg::new("note").long("note")),
        )
}

fn discovery_task() -> Command {
    let lease_arg = || {
        Arg::new("lease-seconds")
            .long("lease-seconds")
            .required(true)
            .value_parser(clap::value_parser!(u64).range(1..=u64::from(DiscoveryLeaseSeconds::MAX)))
    };
    Command::new("task")
        .subcommand_required(true)
        .subcommand(list_leaf("list").arg(Arg::new("pod").long("pod")).arg(
            Arg::new("state").long("state").value_parser([
                "ready",
                "pending",
                "leased",
                "completed",
                "terminal-failure",
            ]),
        ))
        .subcommand(Command::new("show").arg(Arg::new("id").required(true)))
        .subcommand(
            Command::new("claim")
                .arg(Arg::new("id").required(true))
                .arg(lease_arg()),
        )
        .subcommand(
            Command::new("renew")
                .arg(Arg::new("id").required(true))
                .arg(lease_arg()),
        )
        .subcommand(Command::new("complete").arg(Arg::new("id").required(true)))
        .subcommand(
            Command::new("fail")
                .arg(Arg::new("id").required(true))
                .arg(Arg::new("reason").long("reason").required(true)),
        )
}

fn feed() -> Command {
    Command::new("feed")
        .subcommand_required(true)
        .subcommand(
            Command::new("batch")
                .subcommand_required(true)
                .subcommand(Command::new("get").arg(input_arg(false)))
                .subcommand(Command::new("complete").arg(Arg::new("id").required(true))),
        )
        .subcommand(
            Command::new("feedback")
                .subcommand_required(true)
                .subcommand(
                    Command::new("record")
                        .arg(Arg::new("content-item-id").required(true))
                        .arg(
                            Arg::new("kind")
                                .long("kind")
                                .required(true)
                                .value_parser(clap::value_parser!(FeedbackKind)),
                        )
                        .arg(Arg::new("topic").long("topic"))
                        .arg(Arg::new("reason").long("reason")),
                ),
        )
        .subcommand(
            Command::new("taste")
                .subcommand_required(true)
                .subcommand(Command::new("show"))
                .subcommand(Command::new("set").arg(input_arg(true)))
                .subcommand(Command::new("reset").arg(input_arg(false))),
        )
}

fn input_arg(required: bool) -> Arg {
    Arg::new("input")
        .long("input")
        .required(required)
        .value_parser(clap::value_parser!(PathBuf))
        .value_hint(ValueHint::FilePath)
}

fn sync() -> Command {
    Command::new("sync")
        .subcommand_required(true)
        .subcommand(resource("peer", &["list", "add", "remove"]))
        .subcommand(resource("pod", &["run", "status"]))
}

fn resource(name: &'static str, operations: &[&'static str]) -> Command {
    operations.iter().fold(
        Command::new(name).subcommand_required(true),
        |command, operation| {
            command.subcommand(if *operation == "list" {
                list_leaf(operation)
            } else {
                Command::new(operation)
            })
        },
    )
}

fn list_leaf(name: &'static str) -> Command {
    Command::new(name)
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("50")
                .value_parser(clap::value_parser!(u16).range(1..=100)),
        )
        .arg(Arg::new("cursor").long("cursor"))
}
