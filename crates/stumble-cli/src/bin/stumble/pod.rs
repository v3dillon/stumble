use super::{
    agent_tools_error, agent_tools_error_from_store, direct_subscription_error, internal_error,
    page, parse_id, pod_result, read_portable_package_directory, resolve_pod, CliResult,
};
use crate::parser::{
    ContentWorkflow, CreatePodArgs, PackageWorkflow, PodWorkflow, PolicyMode, PolicyWorkflow,
    RoleChangeArgs, RoleWorkflow, SubscriptionWorkflow, VisibilityWorkflow,
};
use serde_json::json;
use stumble_cli::{paginate, ErrorBody, ExitStatusCategory};
use stumble_core::{
    pod_package_contents_from_files, validate_pod_package_contents,
    validate_portable_package_files, AddContentItemToPodRequest, AgentTools, AgentToolsError,
    AuthContext, CandidateConfidence, ContentItemId, CreatePodLifecycleRequest, CreatePodOutcome,
    CreatePodRequest, CurationPolicy, CurationRationale, ExploreRequest, HarnessCapability,
    PackageVersion, PodCreationPackage, PodPackageRevisionOutcome, SensitiveChange, StoreError,
};

pub(super) fn execute(command: PodWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        PodWorkflow::List(args) => {
            let mut pods = tools
                .list_pods_for_harness(actor)
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
            serde_json::to_value(page(items, &args)?).map_err(internal_error)
        }
        PodWorkflow::Show(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let allowed_actions = tools
                .pod_allowed_actions(actor, pod.id)
                .map_err(agent_tools_error)?;
            let mut result = pod_result(pod)?;
            result["allowed_actions"] =
                serde_json::to_value(allowed_actions).map_err(internal_error)?;
            Ok(result)
        }
        PodWorkflow::Create(args) => create_pod(args, tools, actor),
        PodWorkflow::Explore(args) => {
            let request = ExploreRequest::new(
                args.query.unwrap_or_default(),
                50,
                usize::from(args.sample_size),
            )
            .map_err(|error| agent_tools_error(error.into()))?;
            // When Indexes are configured, fan out explicit query via HTTP transport
            // then rank locally. Empty/local-only Explore stays on the Home Node.
            let has_indexes = tools
                .trust_policy(actor)
                .map(|policy| !policy.index_nodes.is_empty())
                .unwrap_or(false);
            let explored = if has_indexes {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .map_err(internal_error)?;
                let client = stumble_api::ReqwestIndexSearchClient::new(runtime.handle().clone());
                let explored = tools
                    .explore_public_pods_with_indexes(actor, request, &client)
                    .map_err(agent_tools_error)?;
                drop(runtime);
                explored
            } else {
                tools
                    .explore_public_pods(actor, request)
                    .map_err(agent_tools_error)?
            };
            let results = paginate(
                explored.results,
                args.page.limit,
                args.page.cursor.as_deref(),
            )
            .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            Ok(json!({
                "query": explored.query,
                "items": results.items,
                "next_cursor": results.next_cursor,
            }))
        }
        PodWorkflow::Subscribe(args) => subscribe(&args.pod, tools, actor),
        PodWorkflow::Unsubscribe(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let subscription = tools
                .unsubscribe_pod(actor, pod.id)
                .map_err(agent_tools_error)?;
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "subscription_id": subscription.id,
                "unsubscribed": true,
            }))
        }
        PodWorkflow::Subscription { command } => match command {
            SubscriptionWorkflow::Set(args) => {
                let pod = resolve_pod(tools, actor, &args.pod)?;
                tools
                    .set_priority_subscription(actor, pod.id, args.priority)
                    .map_err(agent_tools_error)?;
                Ok(json!({
                    "pod_id": pod.id,
                    "slug": pod.slug,
                    "is_priority": args.priority,
                }))
            }
        },
        PodWorkflow::Visibility { command } => match command {
            VisibilityWorkflow::Set(args) => {
                let pod = resolve_pod(tools, actor, &args.pod)?;
                let outcome = tools
                    .request_set_pod_visibility(actor, pod.id, args.visibility, chrono::Utc::now())
                    .map_err(agent_tools_error)?;
                Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "outcome": outcome }))
            }
        },
        PodWorkflow::Role { command } => execute_role(command, tools, actor),
        PodWorkflow::Content { command } => execute_content(command, tools, actor),
        PodWorkflow::Policy { command } => execute_policy(command, tools, actor),
        PodWorkflow::Package { command } => execute_package(command, tools, actor),
    }
}

fn create_pod(args: CreatePodArgs, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    let package = if let Some(path) = args.package {
        let files = read_portable_package_directory(&path)
            .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
        PodCreationPackage::Initial {
            package: pod_package_contents_from_files(&files)
                .map_err(agent_tools_error_from_store)?,
        }
    } else if let Some(reference) = args.from_pod {
        let source = resolve_pod(tools, actor, &reference)?;
        PodCreationPackage::Derived {
            source_package: tools
                .get_skill_pack(actor, &source.slug)
                .map_err(agent_tools_error)?,
        }
    } else {
        PodCreationPackage::Default
    };
    let outcome = tools
        .request_create_pod_lifecycle(
            actor,
            CreatePodLifecycleRequest {
                pod: CreatePodRequest {
                    name: args.name,
                    slug: args.slug,
                    description: args.description.unwrap_or_default(),
                    visibility: args.visibility,
                },
                package,
            },
            chrono::Utc::now(),
        )
        .map_err(agent_tools_error)?;
    match outcome {
        CreatePodOutcome::Created(pod) => Ok(json!({
            "status": "created",
            "result": pod_result(pod)?,
        })),
        CreatePodOutcome::PendingApproval(proposal) => Ok(json!({
            "status": "pending_approval",
            "result": proposal,
        })),
    }
}

fn subscribe(reference: &str, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    if reference.starts_with("https://") || reference.starts_with("http://") {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(internal_error)?;
        let result = runtime
            .block_on(stumble_sync::subscribe_pod_from_url(
                tools, actor, reference,
            ))
            .map_err(direct_subscription_error)?;
        return Ok(json!({
            "pod_id": result.subscription.local_pod_id,
            "slug": result.subscription.pod_slug,
            "subscription": result.subscription,
            "imported_events": result.imported_events,
        }));
    }
    let pod = resolve_pod(tools, actor, reference)?;
    let subscription = tools
        .subscribe_local_pod(actor, pod.id)
        .map_err(agent_tools_error)?;
    Ok(json!({
        "pod_id": pod.id,
        "slug": pod.slug,
        "subscription": subscription,
    }))
}

fn execute_role(command: RoleWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        RoleWorkflow::List(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let roles = tools
                .list_pod_roles(actor, pod.id)
                .map_err(agent_tools_error)?;
            let page = page(roles, &args.page)?;
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "items": page.items,
                "next_cursor": page.next_cursor,
            }))
        }
        RoleWorkflow::Grant(args) => change_role(args, RoleChange::Grant, tools, actor),
        RoleWorkflow::Revoke(args) => change_role(args, RoleChange::Revoke, tools, actor),
    }
}

fn change_role(
    args: RoleChangeArgs,
    change: RoleChange,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    let pod = resolve_pod(tools, actor, &args.pod)?;
    let user_id = parse_id::<uuid::Uuid>(&args.user_id)?;
    let proposal = match change {
        RoleChange::Grant => {
            tools.request_grant_pod_role(actor, pod.id, user_id, args.role, chrono::Utc::now())
        }
        RoleChange::Revoke => {
            tools.request_revoke_pod_role(actor, pod.id, user_id, args.role, chrono::Utc::now())
        }
    }
    .map_err(agent_tools_error)?;
    Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "proposal": proposal }))
}

fn execute_content(command: ContentWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        ContentWorkflow::List(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let items = tools
                .pod_content_stream(actor, pod.id)
                .map_err(agent_tools_error)?;
            let page = page(items, &args.page)?;
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "items": page.items,
                "next_cursor": page.next_cursor,
            }))
        }
        ContentWorkflow::Show(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let content_item_id = parse_id::<ContentItemId>(&args.content_item_id)?;
            let item = tools
                .pod_content_stream(actor, pod.id)
                .map_err(agent_tools_error)?
                .into_iter()
                .find(|item| item.content_item.id() == content_item_id)
                .ok_or_else(|| {
                    (
                        ErrorBody::new("not_found", "Accepted Pod Content Item was not found"),
                        ExitStatusCategory::ValidationOrConflict,
                    )
                })?;
            let allowed_actions = match tools.pod_curation_policy(actor, pod.id) {
                Ok(_) => vec!["remove"],
                Err(AgentToolsError::Forbidden { .. }) => Vec::new(),
                Err(error) => return Err(agent_tools_error(error)),
            };
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "content_item": item.content_item,
                "accepted_placement": item.accepted_placement,
                "allowed_actions": allowed_actions,
            }))
        }
        ContentWorkflow::Add(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let content_item_id = parse_id::<ContentItemId>(&args.content_item_id)?;
            let request = AddContentItemToPodRequest::new(content_item_id, pod.id, args.note)
                .map_err(|error| {
                    agent_tools_error(StoreError::Validation(error.to_string()).into())
                })?;
            let placement = tools
                .add_content_item_to_pod(actor, request, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "placement": placement }))
        }
        ContentWorkflow::Remove(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let content_item_id = parse_id::<ContentItemId>(&args.content_item_id)?;
            let reason = CurationRationale::new(args.reason).map_err(|error| {
                agent_tools_error(StoreError::Validation(error.to_string()).into())
            })?;
            let outcome = tools
                .request_remove_content_item_from_pod(
                    actor,
                    pod.id,
                    content_item_id,
                    reason,
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            let mut result = serde_json::to_value(outcome).map_err(internal_error)?;
            result["pod_id"] = json!(pod.id);
            result["slug"] = json!(pod.slug);
            Ok(result)
        }
    }
}

fn execute_policy(command: PolicyWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        PolicyWorkflow::Show(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let policy = tools
                .pod_curation_policy(actor, pod.id)
                .map_err(agent_tools_error)?;
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "policy": policy,
                "allowed_actions": ["set"],
            }))
        }
        PolicyWorkflow::Set(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let threshold = || {
                CandidateConfidence::new(
                    args.confidence_threshold
                        .expect("required by clap for threshold policies"),
                )
                .map_err(|error| {
                    agent_tools_error(StoreError::Validation(error.to_string()).into())
                })
            };
            match args.mode {
                PolicyMode::Autonomous => {
                    let proposal = tools
                        .create_pending_proposal(
                            actor,
                            SensitiveChange::EnableAutonomousCuration {
                                pod_id: pod.id,
                                confidence_threshold: threshold()?,
                            },
                            chrono::Utc::now(),
                            chrono::Utc::now() + chrono::Duration::hours(24),
                        )
                        .map_err(agent_tools_error)?;
                    Ok(json!({
                        "pod_id": pod.id,
                        "slug": pod.slug,
                        "status": "pending_approval",
                        "proposal": proposal,
                    }))
                }
                PolicyMode::Manual => update_policy(pod, CurationPolicy::Manual, tools, actor),
                PolicyMode::Assisted => update_policy(
                    pod,
                    CurationPolicy::Assisted {
                        confidence_threshold: threshold()?,
                    },
                    tools,
                    actor,
                ),
            }
        }
    }
}

enum RoleChange {
    Grant,
    Revoke,
}

fn update_policy(
    pod: stumble_core::Pod,
    policy: CurationPolicy,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    let policy = tools
        .set_pod_curation_policy(actor, pod.id, policy, chrono::Utc::now())
        .map_err(agent_tools_error)?;
    Ok(json!({
        "pod_id": pod.id,
        "slug": pod.slug,
        "status": "updated",
        "policy": policy,
    }))
}

fn execute_package(command: PackageWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        PackageWorkflow::Show(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let current = tools
                .get_skill_pack(actor, &pod.slug)
                .map_err(agent_tools_error)?;
            let requested = args
                .version
                .map(PackageVersion::new)
                .transpose()
                .map_err(|error| {
                    agent_tools_error(StoreError::Validation(error.to_string()).into())
                })?;
            let package = if let Some(version) = requested {
                tools
                    .get_pod_package_version(actor, &pod.slug, version)
                    .map_err(agent_tools_error)?
            } else {
                current.clone()
            };
            let mut allowed_actions = Vec::new();
            if package.version == current.version {
                allowed_actions.push("export");
                if tools
                    .require_harness_capability(actor, HarnessCapability::PackageManagement)
                    .is_ok()
                {
                    allowed_actions.push("revise");
                }
            }
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "package": package,
                "allowed_actions": allowed_actions,
            }))
        }
        PackageWorkflow::Export(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let export = tools
                .export_skill_pack(actor, &pod.slug)
                .map_err(agent_tools_error)?;
            std::fs::create_dir_all(&args.output).map_err(package_export_error)?;
            for (name, contents) in export.files {
                std::fs::write(args.output.join(name), contents).map_err(package_export_error)?;
            }
            let package = tools
                .get_skill_pack(actor, &pod.slug)
                .map_err(agent_tools_error)?;
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "version": package.version,
                "output": args.output,
            }))
        }
        PackageWorkflow::Validate(args) => {
            let files = read_portable_package_directory(&args.package)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            validate_portable_package_files(&files).map_err(agent_tools_error_from_store)?;
            let contents =
                pod_package_contents_from_files(&files).map_err(agent_tools_error_from_store)?;
            serde_json::to_value(validate_pod_package_contents(&contents)).map_err(internal_error)
        }
        PackageWorkflow::Revise(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let files = read_portable_package_directory(&args.package)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            let base_version = PackageVersion::new(args.base_version).map_err(|error| {
                agent_tools_error(StoreError::Validation(error.to_string()).into())
            })?;
            let outcome = tools
                .request_revise_pod_package(actor, pod.id, base_version, files, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            Ok(match outcome {
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
            })
        }
    }
}

fn package_export_error(error: std::io::Error) -> (ErrorBody, ExitStatusCategory) {
    (
        ErrorBody::new("package_export_failed", error.to_string()),
        ExitStatusCategory::Internal,
    )
}
