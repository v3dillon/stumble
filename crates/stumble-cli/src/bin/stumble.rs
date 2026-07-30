use clap::Parser;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::ExitCode,
};
use stumble_cli::{
    owner_authority_store, paginate, render_text, resolve_existing_data_dir,
    resolve_initialized_data_dir, selected_data_dir, CursorPage, ErrorBody, ErrorEnvelope,
    ExitStatusCategory, OwnerAuthorityStore, ResourceDetail, SuccessEnvelope,
};
use stumble_core::{
    empty_home_node_store, seed_store, AgentTools, AgentToolsError, AuthContext,
    DiscoveryLeaseSeconds, DiscoveryTask, DiscoveryTaskState, FeedBatch, Pod, StoreError,
    TasteProfile, PORTABLE_PACKAGE_FILES,
};

#[path = "stumble/discover.rs"]
mod discover_workflow;
#[path = "stumble/feed.rs"]
mod feed_workflow;
#[path = "stumble/node.rs"]
mod node_workflow;
#[path = "stumble/parser.rs"]
mod parser;
#[path = "stumble/pod.rs"]
mod pod_workflow;
#[path = "stumble/sync.rs"]
mod sync_workflow;
use parser::{Cli, Workflow};

type CliResult = Result<Value, (ErrorBody, ExitStatusCategory)>;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
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

    let format = cli.format;
    let data_dir = match selected_data_dir(cli.data_dir.as_deref()) {
        Ok(data_dir) => data_dir,
        Err(error) => return fail(error, ExitStatusCategory::Internal),
    };
    let owner_authority = owner_authority_store();
    match dispatch(cli.workflow, &data_dir, owner_authority.as_ref()) {
        Ok(data) => succeed(data, &format),
        Err((error, category)) => fail(error, category),
    }
}

fn dispatch(
    workflow: Workflow,
    selected_data_dir: &Path,
    owner_authority: &dyn OwnerAuthorityStore,
) -> CliResult {
    match workflow {
        Workflow::Add(args) => {
            let (data_dir, tools, actor) = open_home_node(selected_data_dir, owner_authority)?;
            let added = tools
                .add_reference(
                    &actor,
                    stumble_core::AddReferenceRequest {
                        url: args.url,
                        pod: args.pod,
                        title: args.title.clone(),
                        summary: args.summary,
                        excerpt: args.excerpt,
                        tags: args.tags,
                        note: args.note,
                        images: args.images.clone(),
                    },
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            let assets = attach_cover_assets(
                &tools,
                &actor,
                &data_dir,
                &added,
                &args.images,
                args.cover.as_deref(),
                args.cover_source,
                args.title.as_deref(),
            )?;
            let mut result = serde_json::to_value(added).map_err(internal_error)?;
            result["assets"] = serde_json::to_value(assets).map_err(internal_error)?;
            Ok(result)
        }
        Workflow::Node { command } => {
            node_workflow::execute(command, selected_data_dir, owner_authority)
        }
        Workflow::Pod { command } => {
            let (_, tools, actor) = open_home_node(selected_data_dir, owner_authority)?;
            pod_workflow::execute(command, &tools, &actor)
        }
        Workflow::Discover { command } => {
            let (_, tools, actor) = open_home_node(selected_data_dir, owner_authority)?;
            discover_workflow::execute(command, &tools, &actor)
        }
        Workflow::Feed { command } => {
            let (_, tools, actor) = open_home_node(selected_data_dir, owner_authority)?;
            feed_workflow::execute(command, &tools, &actor)
        }
        Workflow::Sync { command } => {
            let (_, tools, actor) = open_home_node(selected_data_dir, owner_authority)?;
            sync_workflow::execute(command, &tools, &actor)
        }
    }
}

/// Records cover assets for a freshly added reference: the first page image
/// becomes a reference-only cover, and a local file (typically a generated
/// cover) is copied under the node's media directory so it survives temp
/// cleanup. Both stay local — assets never federate.
#[allow(clippy::too_many_arguments)]
fn attach_cover_assets(
    tools: &AgentTools,
    actor: &AuthContext,
    data_dir: &Path,
    added: &stumble_core::AddedReference,
    images: &[String],
    cover: Option<&Path>,
    cover_source: parser::CoverSource,
    alt_text: Option<&str>,
) -> Result<Vec<stumble_core::SubmissionAsset>, (ErrorBody, ExitStatusCategory)> {
    let submission_id = stumble_core::SubmissionId::from(added.content_item.id());
    let mut assets = Vec::new();
    if let Some(url) = images.first() {
        assets.push(
            tools
                .add_submission_asset(
                    actor,
                    submission_id,
                    stumble_core::RepresentativeImageRequest {
                        source: stumble_core::SubmissionAssetSource::PageImage,
                        url: Some(url.clone()),
                        local_path: None,
                        mime_type: None,
                        alt_text: alt_text.map(str::to_string),
                    },
                )
                .map_err(agent_tools_error)?,
        );
    }
    if let Some(cover) = cover {
        let extension = cover
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("png")
            .to_lowercase();
        let mime_type = match extension.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            _ => "image/png",
        };
        let media_dir = data_dir
            .join("media")
            .join(added.content_item.id().to_string());
        std::fs::create_dir_all(&media_dir).map_err(internal_error)?;
        let stored = media_dir.join(format!("cover.{extension}"));
        std::fs::copy(cover, &stored).map_err(|error| {
            (
                ErrorBody::new(
                    "invalid_cover",
                    format!("could not store cover {}: {error}", cover.display()),
                ),
                ExitStatusCategory::ValidationOrConflict,
            )
        })?;
        let source = match cover_source {
            parser::CoverSource::AiGenerated => stumble_core::SubmissionAssetSource::AiGenerated,
            parser::CoverSource::PageImage => stumble_core::SubmissionAssetSource::PageImage,
            parser::CoverSource::UserProvided => stumble_core::SubmissionAssetSource::UserProvided,
        };
        assets.push(
            tools
                .add_submission_asset(
                    actor,
                    submission_id,
                    stumble_core::RepresentativeImageRequest {
                        source,
                        url: None,
                        local_path: Some(stored.display().to_string()),
                        mime_type: Some(mime_type.to_string()),
                        alt_text: alt_text.map(str::to_string),
                    },
                )
                .map_err(agent_tools_error)?,
        );
    }
    Ok(assets)
}

fn open_home_node(
    selected_data_dir: &Path,
    owner_authority: &dyn OwnerAuthorityStore,
) -> Result<(PathBuf, AgentTools, AuthContext), (ErrorBody, ExitStatusCategory)> {
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
    let tools = AgentTools::open_initialized_home_node(&data_dir)
        .map_err(agent_tools_error)?
        .with_discovery_peer_probe(std::sync::Arc::new(
            stumble_api::ReqwestDiscoveryPeerProbe,
        ));
    let actor = authenticate_actor(&tools, &data_dir, owner_authority)?;
    Ok((data_dir, tools, actor))
}

fn authenticate_actor(
    tools: &AgentTools,
    data_dir: &std::path::Path,
    owner_authority: &dyn OwnerAuthorityStore,
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
    if !owner_authority
        .is_registered(data_dir)
        .map_err(credential_error)?
    {
        return Err((
            ErrorBody::new(
                "owner_credential_not_found",
                "Home Node Owner Credential entry was not found in the credential store",
            ),
            ExitStatusCategory::Authorization,
        ));
    }
    tools.local_owner_auth_context().map_err(agent_tools_error)
}

fn parse_id<T>(value: &str) -> Result<T, (ErrorBody, ExitStatusCategory)>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.parse::<T>().map_err(|error: T::Err| {
        (
            ErrorBody::new("invalid_id", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        )
    })
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
        "reference": inspection.reference,
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
    serde_json::to_value(profile).map_err(internal_error)
}

fn discovery_lease(seconds: u64) -> Result<DiscoveryLeaseSeconds, (ErrorBody, ExitStatusCategory)> {
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

fn peer_sync_error(error: stumble_sync::PeerSyncError) -> (ErrorBody, ExitStatusCategory) {
    match error {
        stumble_sync::PeerSyncError::Core(error) => agent_tools_error(error),
        stumble_sync::PeerSyncError::DirectSubscription(error) => direct_subscription_error(error),
        error => (
            ErrorBody::new("synchronization_failed", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
    }
}

fn peer_sync_failure_is_retryable(error: &stumble_sync::PeerSyncError) -> bool {
    !matches!(
        error,
        stumble_sync::PeerSyncError::IncompatibleProtocol { .. }
            | stumble_sync::PeerSyncError::PublicKeyMismatch
            | stumble_sync::PeerSyncError::NodeIdentityMismatch
            | stumble_sync::PeerSyncError::SubscriptionPeerMismatch
    )
}

fn peer_sync_failure_code(error: &stumble_sync::PeerSyncError) -> &'static str {
    match error {
        stumble_sync::PeerSyncError::IncompatibleProtocol { .. } => "protocol_incompatible",
        stumble_sync::PeerSyncError::PublicKeyMismatch => "public_key_mismatch",
        stumble_sync::PeerSyncError::NodeIdentityMismatch => "node_identity_mismatch",
        stumble_sync::PeerSyncError::SubscriptionPeerMismatch => "subscription_peer_mismatch",
        _ => "synchronization_failed",
    }
}

fn page<T>(
    items: Vec<T>,
    args: &parser::ListArgs,
) -> Result<CursorPage<T>, (ErrorBody, ExitStatusCategory)> {
    paginate(items, args.limit, args.cursor.as_deref())
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))
}

fn initialize_node(
    selected_data_dir: &std::path::Path,
    owner_authority: &dyn OwnerAuthorityStore,
    demo: bool,
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

    let authority_was_registered = owner_authority
        .is_registered(&data_dir)
        .map_err(credential_error)?;
    if !authority_was_registered {
        owner_authority
            .register(&data_dir)
            .map_err(credential_error)?;
    }
    let seed = if demo {
        seed_store
    } else {
        empty_home_node_store
    };
    let tools = match AgentTools::initialize_home_node(&data_dir, seed) {
        Ok(tools) => tools,
        Err(error) => {
            if !authority_was_registered {
                if let Err(cleanup_error) = owner_authority.remove(&data_dir) {
                    let (primary, _) = agent_tools_error(error);
                    return Err((
                        ErrorBody::new(
                            "node_initialization_failed",
                            format!(
                                "{}; additionally failed to remove the Home Node Owner Credential: {cleanup_error}",
                                primary.message
                            ),
                        ),
                        ExitStatusCategory::Internal,
                    ));
                }
            }
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
        AgentToolsError::PersonalDiscoveryIdempotencyConflict => (
            ErrorBody::new("idempotency_conflict", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::PersonalDiscoveryNotReady => (
            ErrorBody::new("personal_discovery_not_ready", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::CandidateHarnessRequired
        | AgentToolsError::CandidateTaskRequired
        | AgentToolsError::CandidateTaskLeaseRequired
        | AgentToolsError::CandidatePackageVersionMismatch => (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::IndexSearch(ref failure) => (
            ErrorBody::new(failure.kind.as_code(), error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        error => internal_error(error),
    }
}

#[cfg(all(test, unix))]
mod owner_authority_initialization_tests {
    use super::initialize_node;
    use std::{os::unix::fs::PermissionsExt, path::Path};
    use stumble_cli::{CredentialStoreError, OwnerAuthorityStore};

    struct RemovalFailureStore;

    impl OwnerAuthorityStore for RemovalFailureStore {
        fn register(&self, _data_dir: &Path) -> Result<(), CredentialStoreError> {
            Ok(())
        }

        fn is_registered(&self, _data_dir: &Path) -> Result<bool, CredentialStoreError> {
            Ok(false)
        }

        fn remove(&self, _data_dir: &Path) -> Result<(), CredentialStoreError> {
            Err(CredentialStoreError::Backend(
                "simulated cleanup failure".into(),
            ))
        }
    }

    #[test]
    fn initialization_reports_both_database_and_authority_cleanup_failures() {
        let data_dir = std::env::temp_dir().join(format!(
            "stumble-owner-cleanup-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o500)).unwrap();

        let (error, _) = initialize_node(&data_dir, &RemovalFailureStore, false).unwrap_err();

        std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
        assert_eq!(error.code, "node_initialization_failed");
        assert!(error.message.contains("storage sqlite failed"));
        assert!(error.message.contains("simulated cleanup failure"));
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
